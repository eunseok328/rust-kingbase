use bytes::{Buf, BufMut, BytesMut};
use rust_decimal_1::Decimal;
use std::error::Error;

use crate::{FromSql, IsNull, ToSql, Type};

const SIGN_POS: u16 = 0x0000;
const SIGN_NEG: u16 = 0x4000;
const SIGN_NAN: u16 = 0xC000;

impl<'a> FromSql<'a> for Decimal {
    fn from_sql(_: &Type, raw: &[u8]) -> Result<Decimal, Box<dyn Error + Sync + Send>> {
        let text = numeric_to_string(raw)?;
        Decimal::from_str_exact(&text)
            .or_else(|error| {
                // NUMERIC can have a larger display scale than Decimal even when
                // its value fits exactly. Only discard fractional trailing zeros.
                if text.contains('.') {
                    Decimal::from_str_exact(text.trim_end_matches('0').trim_end_matches('.'))
                } else {
                    Err(error)
                }
            })
            .map_err(|error| {
                format!("NUMERIC value cannot be represented exactly as Decimal: {error}").into()
            })
    }

    accepts!(NUMERIC);
}

impl ToSql for Decimal {
    fn to_sql(&self, _: &Type, out: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        Numeric::from_decimal(self)?.write(out)?;
        Ok(IsNull::No)
    }

    accepts!(NUMERIC);
    to_sql_checked!();
}

struct Numeric {
    weight: i16,
    sign: u16,
    dscale: i16,
    digits: Vec<i16>,
}

impl Numeric {
    fn from_decimal(value: &Decimal) -> Result<Numeric, Box<dyn Error + Sync + Send>> {
        let mut text = value.to_string();
        let sign = if text.starts_with('-') {
            text.remove(0);
            SIGN_NEG
        } else {
            SIGN_POS
        };

        let (integer, fractional) = match text.split_once('.') {
            Some((integer, fractional)) => (integer, fractional),
            None => (text.as_str(), ""),
        };
        if !integer.bytes().all(|b| b.is_ascii_digit())
            || !fractional.bytes().all(|b| b.is_ascii_digit())
        {
            return Err("invalid DECIMAL value".into());
        }

        let dscale = i16::try_from(fractional.len()).map_err(|_| "NUMERIC scale out of range")?;
        let integer = integer.trim_start_matches('0');
        let integer_len = integer.len();
        let coefficient = format!("{integer}{fractional}");
        if coefficient.trim_matches('0').is_empty() {
            return Ok(Numeric {
                weight: 0,
                sign: SIGN_POS,
                dscale,
                digits: vec![],
            });
        }

        let integer_groups = integer_len.div_ceil(4);
        let mut groups = Vec::new();
        if integer_groups > 0 {
            let mut integer_padded = String::new();
            integer_padded.extend(std::iter::repeat_n('0', integer_groups * 4 - integer_len));
            integer_padded.push_str(integer);
            let first_len = match integer_padded.len() % 4 {
                0 => 4,
                len => len,
            };
            groups.push(parse_group(&integer_padded[..first_len])?);
            for chunk in integer_padded[first_len..].as_bytes().chunks(4) {
                groups.push(parse_group(std::str::from_utf8(chunk)?)?);
            }
        }

        let mut fractional = fractional.to_owned();
        let padding = (4 - fractional.len() % 4) % 4;
        fractional.extend(std::iter::repeat_n('0', padding));
        for chunk in fractional.as_bytes().chunks(4) {
            groups.push(parse_group(std::str::from_utf8(chunk)?)?);
        }

        let mut weight = if integer_groups == 0 {
            -1
        } else {
            i16::try_from(integer_groups - 1).map_err(|_| "NUMERIC weight out of range")?
        };
        while groups.first() == Some(&0) {
            groups.remove(0);
            weight = weight.checked_sub(1).ok_or("NUMERIC weight out of range")?;
        }
        while groups.last() == Some(&0) {
            groups.pop();
        }
        if groups.is_empty() {
            weight = 0;
        }

        Ok(Numeric {
            weight,
            sign,
            dscale,
            digits: groups,
        })
    }

    fn write(&self, out: &mut BytesMut) -> Result<(), Box<dyn Error + Sync + Send>> {
        out.put_i16(i16::try_from(self.digits.len()).map_err(|_| "too many NUMERIC digits")?);
        out.put_i16(self.weight);
        out.put_u16(self.sign);
        out.put_i16(self.dscale);
        for digit in &self.digits {
            out.put_i16(*digit);
        }
        Ok(())
    }
}

fn numeric_to_string(mut raw: &[u8]) -> Result<String, Box<dyn Error + Sync + Send>> {
    if raw.len() < 8 {
        return Err("invalid NUMERIC value".into());
    }
    let ndigits = raw.get_i16();
    let weight = raw.get_i16();
    let sign = raw.get_u16();
    let dscale = raw.get_i16();
    if ndigits < 0 || dscale < 0 || raw.len() != ndigits as usize * 2 {
        return Err("invalid NUMERIC value".into());
    }
    if sign == SIGN_NAN {
        return Err("NaN NUMERIC values are not supported for Decimal".into());
    }
    if sign != SIGN_POS && sign != SIGN_NEG {
        return Err("invalid NUMERIC sign".into());
    }

    let mut digits = Vec::with_capacity(ndigits as usize);
    for _ in 0..ndigits {
        let digit = raw.get_i16();
        if !(0..10000).contains(&digit) {
            return Err("invalid NUMERIC digit".into());
        }
        digits.push(digit);
    }

    let dscale = dscale as usize;
    if digits.is_empty() {
        let mut text = String::from("0");
        append_fractional_scale(&mut text, dscale);
        return Ok(text);
    }

    let decimal_groups = i32::from(weight) + 1;
    let mut text = String::new();
    if sign == SIGN_NEG {
        text.push('-');
    }
    if decimal_groups <= 0 {
        text.push('0');
    } else {
        let groups = usize::try_from(decimal_groups).map_err(|_| "invalid NUMERIC weight")?;
        for i in 0..groups {
            match digits.get(i) {
                Some(digit) if i == 0 => text.push_str(&digit.to_string()),
                Some(digit) => text.push_str(&format!("{digit:04}")),
                None => text.push_str("0000"),
            }
        }
    }

    let mut fractional = String::new();
    if decimal_groups < 0 {
        for _ in 0..-decimal_groups {
            fractional.push_str("0000");
        }
    }
    let start = decimal_groups.max(0) as usize;
    for digit in digits.iter().skip(start) {
        fractional.push_str(&format!("{digit:04}"));
    }
    fractional.truncate(dscale);
    while fractional.len() < dscale {
        fractional.push('0');
    }
    if dscale > 0 {
        text.push('.');
        text.push_str(&fractional);
    }
    Ok(text)
}

fn append_fractional_scale(text: &mut String, scale: usize) {
    if scale > 0 {
        text.push('.');
        text.extend(std::iter::repeat_n('0', scale));
    }
}

fn parse_group(group: &str) -> Result<i16, Box<dyn Error + Sync + Send>> {
    group
        .parse::<i16>()
        .map_err(|_| "invalid NUMERIC digit".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independent NUMERIC wire fixtures, rather than values produced by Decimal's encoder.
    fn numeric(weight: i16, sign: u16, scale: i16, digits: &[i16]) -> BytesMut {
        let mut out = BytesMut::new();
        out.put_i16(digits.len() as i16);
        out.put_i16(weight);
        out.put_u16(sign);
        out.put_i16(scale);
        for digit in digits {
            out.put_i16(*digit);
        }
        out
    }

    fn decimal(value: &str) -> Decimal {
        Decimal::from_str_exact(value).unwrap()
    }

    #[test]
    fn accepts_numeric() {
        assert!(<Decimal as ToSql>::accepts(&Type::NUMERIC));
        assert!(<Decimal as FromSql<'_>>::accepts(&Type::NUMERIC));
    }

    #[test]
    fn round_trips_values() {
        for value in [
            "0",
            "0.00",
            "1",
            "-1",
            "12345.6789",
            "-0.0012",
            "0.000001",
            "10000.0001",
            "-00012.3400",
        ] {
            let value = decimal(value);
            let mut out = BytesMut::new();
            value.to_sql(&Type::NUMERIC, &mut out).unwrap();
            let decoded = Decimal::from_sql(&Type::NUMERIC, &out).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn decodes_exact_precision_boundaries() {
        let maximum = [7, 9228, 1625, 1426, 4337, 5935, 4395, 335];
        for (sign, expected) in [(SIGN_POS, Decimal::MAX), (SIGN_NEG, Decimal::MIN)] {
            let raw = numeric(7, sign, 0, &maximum);
            assert_eq!(Decimal::from_sql(&Type::NUMERIC, &raw).unwrap(), expected);
        }

        let raw = numeric(-7, SIGN_POS, 28, &[1]);
        assert_eq!(
            Decimal::from_sql(&Type::NUMERIC, &raw).unwrap(),
            decimal("0.0000000000000000000000000001")
        );
        let raw = numeric(
            -1,
            SIGN_POS,
            28,
            &[1234, 5678, 9012, 3456, 7890, 1234, 5678],
        );
        assert_eq!(
            Decimal::from_sql(&Type::NUMERIC, &raw).unwrap(),
            decimal("0.1234567890123456789012345678")
        );
    }

    #[test]
    fn rejects_values_that_would_round_or_underflow() {
        let cases: &[(i16, i16, &[i16])] = &[
            (-1, 29, &[1234, 5678, 9012, 3456, 7890, 1234, 5678, 9000]),
            (-8, 29, &[1000]),                                           // 1e-29
            (-33, 130, &[100]),                                          // 1e-130
            (-250, 1000, &[1]),                                          // 1e-1000
            (7, 0, &[7, 9228, 1625, 1426, 4337, 5935, 4395, 336]),       // MAX + 1
            (7, 1, &[7, 9228, 1625, 1426, 4337, 5935, 4395, 335, 1000]), // MAX + 0.1
        ];
        for &(weight, scale, digits) in cases {
            for sign in [SIGN_POS, SIGN_NEG] {
                let raw = numeric(weight, sign, scale, digits);
                let error = Decimal::from_sql(&Type::NUMERIC, &raw).unwrap_err();
                assert!(error.to_string().contains("represented exactly"), "{error}");
            }
        }
    }

    #[test]
    fn accepts_exact_values_with_excess_fractional_zeros() {
        let cases: &[(i16, &[i16], Decimal)] = &[
            (0, &[], Decimal::ZERO),
            (0, &[1, 2300], decimal("1.23")),
            (-7, &[1], decimal("0.0000000000000000000000000001")),
            (
                7,
                &[7, 9228, 1625, 1426, 4337, 5935, 4395, 335],
                Decimal::MAX,
            ),
        ];
        for &(weight, digits, expected) in cases {
            for scale in [29, 130, 1000] {
                for sign in [SIGN_POS, SIGN_NEG] {
                    let raw = numeric(weight, sign, scale, digits);
                    let value = Decimal::from_sql(&Type::NUMERIC, &raw).unwrap();
                    assert_eq!(
                        value,
                        if sign == SIGN_NEG {
                            -expected
                        } else {
                            expected
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn preserves_representable_scale() {
        let raw = numeric(0, SIGN_POS, 4, &[123, 4500]);
        let value = Decimal::from_sql(&Type::NUMERIC, &raw).unwrap();
        assert_eq!(value.to_string(), "123.4500");
        assert_eq!(value.scale(), 4);
    }
}
