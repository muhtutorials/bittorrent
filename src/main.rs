use serde_json::{Map, Value};
use std::env;
// use serde_bencode

#[allow(dead_code)]
fn decode_bencoded_value(encoded_value: &str) -> (Value, &str) {
    match encoded_value.chars().next().unwrap() {
        'i' => {
            if let Some((n, rest)) = encoded_value
                .strip_prefix('i')
                .and_then(|rest| rest.split_once('e'))
                .and_then(|(digits, rest)| {
                    let n = digits.parse::<i64>().ok()?;
                    Some((n, rest))
                })
            {
                (n.into(), rest)
            } else {
                panic!("Unhandled encoded value: {}", encoded_value);
            }
        }
        'l' => {
            let mut rest = encoded_value.strip_prefix("l").unwrap();
            let mut values = Vec::new();
            while !rest.is_empty() && !rest.starts_with("e") {
                let (value, remainder) = decode_bencoded_value(rest);
                values.push(value);
                rest = remainder
            }
            (values.into(), rest)
        }
        'd' => {
            let mut rest = encoded_value.strip_prefix("d").unwrap();
            let mut dict = Map::new();
            while !rest.is_empty() && !rest.starts_with("e") {
                let (key, remainder) = decode_bencoded_value(rest);
                if let Value::String(key) = key {
                    let (value, remainder) = decode_bencoded_value(remainder);
                    dict.insert(key, value);
                    rest = remainder
                } else {
                    panic!("dict keys must be strings")
                }
            }
            (dict.into(), rest)
        }
        '0'..='9' => {
            if let Some((len, rest)) = encoded_value.split_once(':').and_then(|(len, rest)| {
                let len = len.parse::<usize>().ok()?;
                Some((len, rest))
            }) {
                (rest[..len].to_string().into(), &rest[len..])
            } else {
                panic!("Unhandled encoded value: {}", encoded_value);
            }
        }
        _ => panic!("Unhandled encoded value: {}", encoded_value),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = &args[1];
    if command == "decode" {
        let encoded_value = &args[2];
        let decoded_value = decode_bencoded_value(encoded_value);
        println!("{}", decoded_value.0.to_string());
    } else {
        println!("unknown command: {}", args[1])
    }
}