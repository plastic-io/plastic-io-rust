use rusty_v8 as v8;
use lazy_static::lazy_static;
use serde_json::Value;
use std::sync::{Arc, Mutex};

lazy_static! {
  static ref SEQUENCE_COUNTER: Arc<Mutex<u32>> = Arc::new(Mutex::new(0));
}

pub fn log(category: &str, msg: String) {
  println!("------------------------------------------------------------------------------------------------------------");
  println!("{}", category);
  println!("Sequence: {}", get_seq_count());
  println!("{}", msg.replace(",", "\n"));
}


pub fn increment_sequence_counter() {
  let mut sequence_counter = SEQUENCE_COUNTER.lock().expect("Could not lock sequence number");
  *sequence_counter += 1;
}

pub fn get_seq_count() -> u32 {
  let sequence_counter = SEQUENCE_COUNTER.lock().expect("Could not lock sequence number");
  return sequence_counter.clone();
}

pub fn serde_json_to_v8<'a>(
  scope: &mut v8::HandleScope<'a>,
  value: &Value,
) -> v8::Local<'a, v8::Value> {
  let err = "Failed to convert JSON to v8 value";
  match value {
      Value::Null => v8::null(scope).into(),
      Value::Bool(b) => v8::Boolean::new(scope, *b).into(),
      Value::Number(num) => {
          if let Some(n) = num.as_f64() {
              v8::Number::new(scope, n).into()
          } else {
              v8::undefined(scope).into()
          }
      }
      Value::String(s) => v8::String::new(scope, s).expect(err).into(),
      Value::Array(arr) => {
          let array = v8::Array::new(scope, arr.len() as i32);
          for (i, item) in arr.iter().enumerate() {
              let v8_item = serde_json_to_v8(scope, item);
              array.set_index(scope, i as u32, v8_item);
          }
          array.into()
      }
      Value::Object(obj) => {
          let object = v8::Object::new(scope);
          for (k, v) in obj {
              let key = v8::String::new(scope, k).expect(err).into();
              let value = serde_json_to_v8(scope, v);
              object.set(scope, key, value).expect(err);
          }
          object.into()
      }
  }
}

pub fn v8_value_to_serde_json(
  value: v8::Local<v8::Value>,
  scope: &mut v8::HandleScope,
) -> serde_json::Value {
  let err = "Failed to convert v8 value to JSON";
  if value.is_string() {
      let value = value.to_rust_string_lossy(scope);
      return serde_json::Value::String(value);
  } else if value.is_number() {
      let num = value.to_number(scope).expect(err).value();
      // JavaScript's Number is always a double-precision floating-point format (f64 in Rust)
      if num.fract() == 0.0 {
          // Check if it can be safely represented as an i64
          if num >= i64::MIN as f64 && num <= i64::MAX as f64 {
              serde_json::Value::Number(serde_json::Number::from(num as i64))
          } else {
              // Outside i64 range, keep as f64
              serde_json::to_value(num).unwrap_or(serde_json::Value::Null)
          }
      } else {
          // For non-integer numbers, represent as f64 directly
          serde_json::to_value(num).unwrap_or(serde_json::Value::Null)
      }
  } else if value.is_boolean() {
      let boolean = value.is_true();
      return serde_json::Value::Bool(boolean);
  } else if value.is_null() {
      return serde_json::Value::Null;
  } else if value.is_undefined() {
      // `undefined` is not directly representable in JSON;
      // you might choose to use null or some other convention
      return serde_json::Value::Null;
  } else {
      // Handle arrays, objects, or other types as needed
      return serde_json::Value::Null; // Placeholder for simplicity
  }
}
