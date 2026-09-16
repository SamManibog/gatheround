use std::fmt::Debug;

use mlua::{Table, Value};

pub trait IColor: Clone + Copy + Debug + Sized {
    fn r(&self) -> u8;
    fn g(&self) -> u8;
    fn b(&self) -> u8;
    fn a(&self) -> u8;

    fn from_other<C: IColor>(other: &C) -> Self {
        Self::from_rgba(other.r(), other.g(), other.b(), other.a())
    }

    fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self;

    fn try_from_hex_str(hex: &str) -> Option<Self> {
        if hex.is_empty() {
            return None;
        }

        let hex = if hex.chars().next().unwrap() == '#' {
            &hex[1..]
        } else {
            hex
        };

        if !hex.is_ascii() {
            return None;
        }

        if hex.len() == 8 {
            let mut out = [0; 4];

            for i in 0..4 {
                out[i] = match u8::from_str_radix(&hex[(i * 2)..(i * 2 + 2)], 16) {
                    Ok(u) => u,
                    Err(_) => return None,
                };
            }

            Some(Self::from_rgba(out[0], out[1], out[2], out[3]))
        } else if hex.len() == 6 {
            let mut out = [0; 3];

            for i in 0..3 {
                out[i] = match u8::from_str_radix(&hex[(i * 2)..(i * 2 + 2)], 16) {
                    Ok(u) => u,
                    Err(_) => return None,
                };
            }

            Some(Self::from_rgba(out[0], out[1], out[2], 255))
        } else {
            None
        }
    }
}

/// attempt to get a color from a lua value
pub fn color_from_lua_value<C: IColor>(value: &Value) -> Result<C, String> {
    match value {
        Value::String(s) => {
            let rust_str = s.to_string_lossy();
            if let Some(c) = C::try_from_hex_str(&rust_str) {
                Ok(c)
            } else {
                Err(format!("Invalid color hex values: {rust_str}"))
            }
        }
        Value::Table(t) => color_from_lua_table(&t),
        _ => Err(format!("Invalid color: expected table or string value.")),
    }
}

/// creates a channel value from a lua value, saturating at u8 buonds
fn channel_from_lua(value: &mlua::Value) -> Result<u8, String> {
    if let mlua::Value::Integer(int) = value {
        Ok((*int).clamp(u8::MIN as i64, u8::MAX as i64) as u8)
    } else if let mlua::Value::Number(float) = value {
        Ok((float.clamp(0.0, 1.0) * u8::MAX as f64).round() as u8)
    } else {
        Err("Non numeric channel field.".to_string())
    }
}

/// attempt to get a color from a lua table
pub fn color_from_lua_table<C: IColor>(table: &Table) -> Result<C, String> {
    let r = match table.raw_get("r") {
        Ok(val) => match channel_from_lua(&val) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid r channel: {msg}")),
        },
        Err(_) => return Err(format!("Invalid r channel.")),
    };

    let g = match table.raw_get("g") {
        Ok(val) => match channel_from_lua(&val) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid g channel: {msg}")),
        },
        Err(_) => return Err(format!("Invalid g channel.")),
    };

    let b = match table.raw_get("b") {
        Ok(val) => match channel_from_lua(&val) {
            Ok(v) => v,
            Err(msg) => return Err(format!("Invalid b channel: {msg}")),
        },
        Err(_) => return Err(format!("Invalid b channel.")),
    };

    let a = match table.raw_get("a") {
        Ok(val) => {
            if val == mlua::Value::Nil {
                255
            } else {
                match channel_from_lua(&val) {
                    Ok(v) => v,
                    Err(msg) => return Err(format!("Invalid r channel: {msg}")),
                }
            }
        }
        Err(_) => return Err(format!("Invalid a channel.")),
    };

    Ok(C::from_rgba(r, g, b, a))
}
