use mlua::Table;

pub trait IColor: Clone {
    fn r(&self) -> u8;
    fn g(&self) -> u8;
    fn b(&self) -> u8;
    fn a(&self) -> u8;

    fn from_other<C: IColor>(other: &C) -> Self {
        Self::from_rgba(other.r(), other.g(), other.b(), other.a())
    }

    fn from_rgba(r: u8, g: u8, b: u8, a: u8) -> Self;
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
