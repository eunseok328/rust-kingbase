// ═══════════════════ [修改开始] Kingbase 引入连接级兼容状态写入能力 ═══════════════════
// 原代码保留：
// use crate::client::{Addr, SocketConfig};
use crate::client::{Addr, SocketConfig};
// ═══════════════════ [修改结束] Kingbase 引入连接级兼容状态写入能力 ═══════════════════
use crate::config::{Host, LoadBalanceHosts, TargetSessionAttrs};
use crate::connect_raw::connect_raw;
use crate::connect_socket::connect_socket;
// ═══════════════════ [新增开始] Kingbase 探测错误策略需要按 SQLSTATE 分类 ═══════════════════
use crate::error::SqlState;
// ═══════════════════ [新增结束] Kingbase 探测错误策略需要按 SQLSTATE 分类 ═══════════════════
use crate::tls::MakeTlsConnect;
use crate::{Client, Config, Connection, Error, SimpleQueryMessage, Socket};
use futures_util::{FutureExt, Stream};
use rand::seq::SliceRandom;
// ═══════════════════ [新增开始] Kingbase 兼容模式探测需要缓存服务端设置 ═══════════════════
use std::collections::HashMap;
// ═══════════════════ [新增结束] Kingbase 兼容模式探测需要缓存服务端设置 ═══════════════════
use std::future::{self, Future};
use std::pin::pin;
use std::task::Poll;
use std::{cmp, io};
use tokio::net;

pub async fn connect<T>(
    mut tls: T,
    config: &Config,
) -> Result<(Client, Connection<Socket, T::Stream>), Error>
where
    T: MakeTlsConnect<Socket>,
{
    if config.host.is_empty() && config.hostaddr.is_empty() {
        return Err(Error::config("both host and hostaddr are missing".into()));
    }

    if !config.host.is_empty()
        && !config.hostaddr.is_empty()
        && config.host.len() != config.hostaddr.len()
    {
        let msg = format!(
            "number of hosts ({}) is different from number of hostaddrs ({})",
            config.host.len(),
            config.hostaddr.len(),
        );
        return Err(Error::config(msg.into()));
    }

    // At this point, either one of the following two scenarios could happen:
    // (1) either config.host or config.hostaddr must be empty;
    // (2) if both config.host and config.hostaddr are NOT empty; their lengths must be equal.
    let num_hosts = cmp::max(config.host.len(), config.hostaddr.len());

    if config.port.len() > 1 && config.port.len() != num_hosts {
        return Err(Error::config("invalid number of ports".into()));
    }

    let mut indices = (0..num_hosts).collect::<Vec<_>>();
    if config.load_balance_hosts == LoadBalanceHosts::Random {
        indices.shuffle(&mut rand::rng());
    }

    let mut error = None;
    for i in indices {
        let host = config.host.get(i);
        let hostaddr = config.hostaddr.get(i);
        let port = config
            .port
            .get(i)
            .or_else(|| config.port.first())
            .copied()
            .unwrap_or(5432);

        // The value of host is used as the hostname for TLS validation,
        let hostname = match host {
            Some(Host::Tcp(host)) => Some(host.clone()),
            // postgres doesn't support TLS over unix sockets, so the choice here doesn't matter
            #[cfg(unix)]
            Some(Host::Unix(_)) => None,
            None => None,
        };

        // Try to use the value of hostaddr to establish the TCP connection,
        // fallback to host if hostaddr is not present.
        let addr = match hostaddr {
            Some(ipaddr) => Host::Tcp(ipaddr.to_string()),
            None => host.cloned().unwrap(),
        };

        match connect_host(addr, hostname, port, &mut tls, config).await {
            Ok((client, connection)) => return Ok((client, connection)),
            Err(e) => error = Some(e),
        }
    }

    Err(error.unwrap())
}

async fn connect_host<T>(
    host: Host,
    hostname: Option<String>,
    port: u16,
    tls: &mut T,
    config: &Config,
) -> Result<(Client, Connection<Socket, T::Stream>), Error>
where
    T: MakeTlsConnect<Socket>,
{
    match host {
        Host::Tcp(host) => {
            let mut addrs = net::lookup_host((&*host, port))
                .await
                .map_err(Error::connect)?
                .collect::<Vec<_>>();

            if config.load_balance_hosts == LoadBalanceHosts::Random {
                addrs.shuffle(&mut rand::rng());
            }

            let mut last_err = None;
            for addr in addrs {
                match connect_once(Addr::Tcp(addr.ip()), hostname.as_deref(), port, tls, config)
                    .await
                {
                    Ok(stream) => return Ok(stream),
                    Err(e) => {
                        last_err = Some(e);
                        continue;
                    }
                };
            }

            Err(last_err.unwrap_or_else(|| {
                Error::connect(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "could not resolve any addresses",
                ))
            }))
        }
        #[cfg(unix)]
        Host::Unix(path) => {
            connect_once(Addr::Unix(path), hostname.as_deref(), port, tls, config).await
        }
    }
}

async fn connect_once<T>(
    addr: Addr,
    hostname: Option<&str>,
    port: u16,
    tls: &mut T,
    config: &Config,
) -> Result<(Client, Connection<Socket, T::Stream>), Error>
where
    T: MakeTlsConnect<Socket>,
{
    let socket = connect_socket(
        &addr,
        port,
        config.connect_timeout,
        config.tcp_user_timeout,
        if config.keepalives {
            Some(&config.keepalive_config)
        } else {
            None
        },
    )
    .await?;

    let tls = tls
        .make_tls_connect(hostname.unwrap_or(""))
        .map_err(|e| Error::tls(e.into()))?;
    let has_hostname = hostname.is_some();
    let (mut client, mut connection) = connect_raw(socket, tls, has_hostname, config).await?;

    // ═══════════════════ [新增开始] Kingbase 四模式连接级探测 ═══════════════════
    // Java Kingbase JDBC connects first, then reads `pg_settings.database_mode`
    // and related settings to decide whether the connection is pg/oracle/mysql/
    // sqlserver compatible. Do the same before exposing the client to callers.
    probe_kingbase_compatibility(&client, &mut connection).await?;
    // ═══════════════════ [新增结束] Kingbase 四模式连接级探测 ═══════════════════

    if config.target_session_attrs != TargetSessionAttrs::Any {
        let mut rows = pin!(client.simple_query_raw("SHOW transaction_read_only"));

        let mut rows = pin!(
            future::poll_fn(|cx| {
                if connection.poll_unpin(cx)?.is_ready() {
                    return Poll::Ready(Err(Error::closed()));
                }

                rows.as_mut().poll(cx)
            })
            .await?
        );

        loop {
            let next = future::poll_fn(|cx| {
                if connection.poll_unpin(cx)?.is_ready() {
                    return Poll::Ready(Some(Err(Error::closed())));
                }

                rows.as_mut().poll_next(cx)
            });

            match next.await.transpose()? {
                Some(SimpleQueryMessage::Row(row)) => {
                    let read_only_result = row.try_get(0)?;
                    if read_only_result == Some("on")
                        && config.target_session_attrs == TargetSessionAttrs::ReadWrite
                    {
                        return Err(Error::connect(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "database does not allow writes",
                        )));
                    } else if read_only_result == Some("off")
                        && config.target_session_attrs == TargetSessionAttrs::ReadOnly
                    {
                        return Err(Error::connect(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "database is not read only",
                        )));
                    } else {
                        break;
                    }
                }
                Some(_) => {}
                None => return Err(Error::unexpected_message()),
            }
        }
    }

    client.set_socket_config(SocketConfig {
        addr,
        hostname: hostname.map(|s| s.to_string()),
        port,
        connect_timeout: config.connect_timeout,
        tcp_user_timeout: config.tcp_user_timeout,
        keepalive: if config.keepalives {
            Some(config.keepalive_config.clone())
        } else {
            None
        },
    });

    Ok((client, connection))
}

// ═══════════════════ [新增开始] Kingbase 四模式兼容探测实现 ═══════════════════
// ═══════════════════ [新增开始] Kingbase 探测错误窄 allowlist 策略 ═══════════════════
fn should_ignore_kingbase_probe_error(error: &Error) -> bool {
    matches!(
        error.code(),
        Some(code)
            if code == &SqlState::UNDEFINED_TABLE
                || code == &SqlState::UNDEFINED_COLUMN
                || code == &SqlState::INSUFFICIENT_PRIVILEGE
                || code == &SqlState::FEATURE_NOT_SUPPORTED
    )
}
// ═══════════════════ [新增结束] Kingbase 探测错误窄 allowlist 策略 ═══════════════════

async fn probe_kingbase_compatibility<T>(
    client: &Client,
    connection: &mut Connection<Socket, T>,
) -> Result<(), Error>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    const KINGBASE_COMPATIBILITY_QUERY: &str = "\
SELECT name, setting
FROM pg_settings
WHERE name = 'database_mode'
   OR name = 'ora_input_emptystr_isnull'
   OR name = 'enable_unpaired_comment'
   OR name = 'sql_mode'
   OR name = 'kdb_flashback.db_recyclebin'";

    let mut settings = HashMap::new();
    let mut rows = pin!(client.simple_query_raw(KINGBASE_COMPATIBILITY_QUERY));

    let rows = match future::poll_fn(|cx| {
        if connection.poll_unpin(cx)?.is_ready() {
            return Poll::Ready(Err(Error::closed()));
        }

        rows.as_mut().poll(cx)
    })
    .await
    {
        Ok(rows) => rows,
        // Match the Java driver's forgiving startup behavior: if the probe is
        // not available on a server, keep the default Pg compatibility mode.
        // ═══════════════════ [修改开始] Kingbase 探测启动错误只忽略允许的数据库 SQLSTATE ═══════════════════
        // 原新增探测逻辑中的宽泛回退：
        // Err(_) => return Ok(()),
        Err(e) if should_ignore_kingbase_probe_error(&e) => return Ok(()),
        Err(e) => return Err(e),
        // ═══════════════════ [修改结束] Kingbase 探测启动错误只忽略允许的数据库 SQLSTATE ═══════════════════
    };
    let mut rows = pin!(rows);

    loop {
        let next = future::poll_fn(|cx| {
            if connection.poll_unpin(cx)?.is_ready() {
                return Poll::Ready(Some(Err(Error::closed())));
            }

            rows.as_mut().poll_next(cx)
        });

        match next.await.transpose() {
            Ok(Some(SimpleQueryMessage::Row(row))) => {
                let name = row.try_get(0)?;
                let setting = row.try_get(1)?;
                if let (Some(name), Some(setting)) = (name, setting) {
                    settings.insert(name.to_string(), setting.to_string());
                }
            }
            Ok(Some(_)) => {}
            Ok(None) => {
                client.inner().set_kingbase_server_settings(settings);
                return Ok(());
            }
            // Keep startup tolerant if the compatibility settings query fails.
            // ═══════════════════ [修改开始] Kingbase 探测流错误只忽略允许的数据库 SQLSTATE ═══════════════════
            // 原新增探测逻辑中的宽泛回退：
            // Err(_) => return Ok(()),
            Err(e) if should_ignore_kingbase_probe_error(&e) => return Ok(()),
            Err(e) => return Err(e),
            // ═══════════════════ [修改结束] Kingbase 探测流错误只忽略允许的数据库 SQLSTATE ═══════════════════
        }
    }
}
// ═══════════════════ [新增结束] Kingbase 四模式兼容探测实现 ═══════════════════
