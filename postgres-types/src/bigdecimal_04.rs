use bigdecimal_04::BigDecimal;
use bytes::{Buf, BufMut, BytesMut};
use std::error::Error;

use crate::{FromSql, IsNull, ToSql, Type};

const SIGN_POS: u16 = 0x0000;
const SIGN_NEG: u16 = 0x4000;
const SIGN_NAN: u16 = 0xC000;

impl<'a> FromSql<'a> for BigDecimal {
    fn from_sql(_: &Type, raw: &[u8]) -> Result<BigDecimal, Box<dyn Error + Sync + Send>> {
        let text = numeric_to_string(raw)?;
        BigDecimal::parse_bytes(text.as_bytes(), 10).ok_or_else(|| "invalid NUMERIC value".into())
    }

    accepts!(NUMERIC);
}

impl ToSql for BigDecimal {
    fn to_sql(&self, _: &Type, w: &mut BytesMut) -> Result<IsNull, Box<dyn Error + Sync + Send>> {
        let numeric = Numeric::try_from(self)?;
        numeric.write(w)?;
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
    fn try_from(value: &BigDecimal) -> Result<Numeric, Box<dyn Error + Sync + Send>> {
        let (bigint, exponent) = value.as_bigint_and_exponent();
        let mut coefficient = bigint.to_string();
        let sign = if coefficient.starts_with('-') {
            coefficient.remove(0);
            SIGN_NEG
        } else {
            SIGN_POS
        };

        let mut scale = exponent;
        if scale < 0 {
            let zeros = usize::try_from(scale.checked_neg().ok_or("NUMERIC scale out of range")?)
                .map_err(|_| "NUMERIC scale out of range")?;
            coefficient.extend(std::iter::repeat_n('0', zeros));
            scale = 0;
        }

        let dscale = i16::try_from(scale).map_err(|_| "NUMERIC scale out of range")?;
        let scale = usize::try_from(scale).map_err(|_| "NUMERIC scale out of range")?;
        let coefficient = coefficient.trim_start_matches('0');

        if coefficient.is_empty() {
            return Ok(Numeric {
                weight: 0,
                sign: SIGN_POS,
                dscale,
                digits: vec![],
            });
        }

        let integer_len = coefficient.len().saturating_sub(scale);
        let mut groups = vec![];

        if integer_len > 0 {
            let integer = &coefficient[..integer_len];
            let first_len = match integer.len() % 4 {
                0 => 4,
                len => len,
            };

            groups.push(parse_group(&integer[..first_len])?);
            for chunk in integer[first_len..].as_bytes().chunks(4) {
                groups.push(parse_group(std::str::from_utf8(chunk)?)?);
            }
        }

        let mut fractional = String::new();
        if scale > coefficient.len() {
            fractional.extend(std::iter::repeat_n('0', scale - coefficient.len()));
            fractional.push_str(coefficient);
        } else if scale > 0 {
            fractional.push_str(&coefficient[coefficient.len() - scale..]);
        }

        let fractional_len = fractional.len();
        if fractional_len > 0 {
            let padding = (4 - fractional_len % 4) % 4;
            fractional.extend(std::iter::repeat_n('0', padding));
            for chunk in fractional.as_bytes().chunks(4) {
                groups.push(parse_group(std::str::from_utf8(chunk)?)?);
            }
        }

        let integer_groups = if integer_len == 0 {
            0
        } else {
            (integer_len + 3) / 4
        };
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
        return Err("NaN NUMERIC values are not supported for BigDecimal".into());
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

    if fractional.len() > dscale {
        fractional.truncate(dscale);
    }
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
    use std::str::FromStr;

    fn decimal(value: &str) -> BigDecimal {
        BigDecimal::from_str(value).unwrap()
    }

    fn encode(value: &str) -> Vec<u8> {
        let mut out = BytesMut::new();
        decimal(value).to_sql(&Type::NUMERIC, &mut out).unwrap();
        out.to_vec()
    }

    #[test]
    fn accepts_numeric_types() {
        assert!(<BigDecimal as ToSql>::accepts(&Type::NUMERIC));
        assert!(<BigDecimal as FromSql<'_>>::accepts(&Type::NUMERIC));
    }

    #[test]
    fn encodes_known_values() {
        assert_eq!(
            encode("12345.6789"),
            [
                0, 3, // ndigits
                0, 1, // weight
                0, 0, // sign
                0, 4, // dscale
                0, 1, 9, 41, 26, 133, // 1, 2345, 6789
            ]
        );
        assert_eq!(
            encode("-0.0012"),
            [
                0, 1, // ndigits
                255, 255, // weight = -1
                64, 0, // sign
                0, 4, // dscale
                0, 12, // digit
            ]
        );
        assert_eq!(encode("0.00"), [0, 0, 0, 0, 0, 0, 0, 2]);
    }

    #[test]
    fn decodes_known_values() {
        assert_eq!(
            BigDecimal::from_sql(
                &Type::NUMERIC,
                &[0, 3, 0, 1, 0, 0, 0, 4, 0, 1, 9, 41, 26, 133]
            )
            .unwrap()
            .to_string(),
            "12345.6789"
        );
        assert_eq!(
            BigDecimal::from_sql(&Type::NUMERIC, &[0, 1, 255, 255, 64, 0, 0, 4, 0, 12])
                .unwrap()
                .to_string(),
            "-0.0012"
        );
    }

    #[test]
    fn round_trips_basic_values() {
        for value in [
            "0",
            "0.00",
            "1",
            "-1",
            "12345.6789",
            "-12345.6789",
            "0.00000123",
            "-0.0012",
            "100000000000000000000.0000000001",
        ] {
            let value = decimal(value);
            let mut out = BytesMut::new();
            value.to_sql(&Type::NUMERIC, &mut out).unwrap();
            let decoded = BigDecimal::from_sql(&Type::NUMERIC, &out).unwrap();
            assert_eq!(decoded, value);
        }
    }

    #[test]
    fn nan_is_not_supported() {
        let err = BigDecimal::from_sql(&Type::NUMERIC, &[0, 0, 0, 0, 192, 0, 0, 0]).unwrap_err();
        assert!(err.to_string().contains("NaN"));
    }
}
