use serde_json::{Map, Value};

pub fn pick(params: &Map<String, Value>, names: Vec<&str>) -> Result<Map<String, Value>, String> {
    let mut values = Map::new();
    for name in names.into_iter() {
        if params.get(name).is_none() {
            return Err(format!("{} does not belong to map", name));
        } else {
            values.insert(String::from(name), params.get(name).unwrap().clone());
        }
    }
    Ok(values)
}

pub fn unwrap<'a>(params: &'a Map<String, Value>, name: &'a str) -> Result<&'a Value, String> {
    let opt_val = params.get(name);
    match opt_val {
        None => {
            Err(format!("{} does not exist", name))
        }
        Some(val) => {
            Ok(val)
        }
    }
}

pub fn get_str<'a>(params: &'a Map<String, Value>, name: &'a str) -> Result<&'a str, String> {
    let unwrapped = unwrap(params, name);
    if unwrapped.is_ok() {
        let opt_val = unwrapped.unwrap().as_str();
        match opt_val {
            None => {
                Err(format!("{} is not {}", name, "str"))
            }
            Some(val) => {
                Ok(val)
            }
        }
    } else {
        Err(unwrapped.unwrap_err())
    }
}

pub fn get_string(params: &Map<String, Value>, name: &str) -> Result<String, String> {
    let result = get_str(params, name);
    if result.is_ok() {
        Ok(String::from(result.unwrap()))
    } else {
        Err(result.unwrap_err())
    }
}

pub fn get_u64(params: &Map<String, Value>, name: &str) -> Result<u64, String> {
    let unwrapped = unwrap(params, name);
    if unwrapped.is_ok() {
        let opt_val = unwrapped.unwrap().as_u64();
        match opt_val {
            None => {
                Err(format!("{} is not {}", name, "u64"))
            }
            Some(val) => {
                Ok(val)
            }
        }
    } else {
        Err(unwrapped.unwrap_err())
    }
}

pub fn get_object<'a>(params: &'a Map<String, Value>, name: &'a str) -> Result<&'a Map<String, Value>, String> {
    let unwrapped = unwrap(params, name);
    if unwrapped.is_ok() {
        let opt_val = unwrapped.unwrap().as_object();
        match opt_val {
            None => {
                Err(format!("{} is not {}", name, "object"))
            }
            Some(val) => {
                Ok(val)
            }
        }
    } else {
        Err(unwrapped.unwrap_err())
    }
}

pub fn get_array<'a>(params: &'a Map<String, Value>, name: &'a str) -> Result<&'a Vec<Value>, String> {
    let unwrapped = unwrap(params, name);
    if unwrapped.is_ok() {
        let opt_val = unwrapped.unwrap().as_array();
        match opt_val {
            None => {
                Err(format!("{} is not {}", name, "array"))
            }
            Some(val) => {
                Ok(val)
            }
        }
    } else {
        Err(unwrapped.unwrap_err())
    }
}

// pub fn get_bool(params: &Map<String, Value>, name: &str) -> Result<bool, String> {
//     let unwrapped = unwrap(params, name);
//     if unwrapped.is_ok() {
//         let opt_val = unwrapped.unwrap().as_bool();
//         match opt_val {
//             None => {
//                 Err(format!("{} is not {}", name, "bool"))
//             }
//             Some(val) => {
//                 Ok(val)
//             }
//         }
//     } else {
//         Err(unwrapped.unwrap_err())
//     }
// }

pub fn get_string_vec(params: &Map<String, Value>, name: &str) -> Vec<String> {
    params.get(name).unwrap().as_array().unwrap().iter().map(|item| { String::from(item.as_str().unwrap()) }).collect()
}

pub fn get_type(value: &Value) -> Result<String, String> {
    if value.is_string() {
        Ok(String::from("string"))
    } else if value.is_u64() {
        Ok(String::from("u64"))
    } else if value.is_i64() {
        Ok(String::from("i64"))
    } else if value.is_f64() {
        Ok(String::from("f64"))
    } else if value.is_number() {
        Ok(String::from("number"))
    } else if value.is_object() {
        Ok(String::from("object"))
    } else if value.is_array() {
        Ok(String::from("array"))
    } else if value.is_boolean() {
        Ok(String::from("bool"))
    } else if value.is_null() {
        Ok(String::from("null"))
    } else {
        Err(String::from("not supported value type"))
    }
}