#[cfg(target_arch = "wasm32")]
use super::wasm::global_property;
use std::char;
use wasm_bindgen::prelude::*;

#[derive(Debug)]
pub enum UuidError {
    NoCryptoGetRandomValues(JsValue),
}

pub fn uuid() -> Result<String, UuidError> {
    let mut numbers = [0u8; 36];
    let _ = make_random_numbers(&mut numbers)?;
    Ok(uuid_from_numbers(&numbers))
}

#[cfg(target_arch = "wasm32")]
pub fn make_random_numbers(numbers: &mut [u8]) -> Result<(), UuidError> {
    use UuidError::*;
    global_property::<web_sys::Crypto>("crypto")
        .map_err(NoCryptoGetRandomValues)?
        .get_random_values_with_u8_array(numbers)
        .map_err(NoCryptoGetRandomValues)?;
    Ok(())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn make_random_numbers(numbers: &mut [u8]) -> Result<(), UuidError> {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    for v in numbers.iter_mut() {
        *v = rng.gen();
    }
    Ok(())
}

enum UuidElements {
    Random09AF,
    Random89AB,
    Hyphen,
    Version,
}

const UUID_V4_FORMAT: [UuidElements; 36] = [
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Hyphen,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Hyphen,
    UuidElements::Version,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Hyphen,
    UuidElements::Random89AB,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Hyphen,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
    UuidElements::Random09AF,
];

const ERROR_MAKE_CHAR: &str = "Error in making char";

pub fn uuid_from_numbers(random_numbers: &[u8; 36]) -> String {
    UUID_V4_FORMAT
        .iter()
        .enumerate()
        .map(|(i, kind)| match kind {
            UuidElements::Random09AF => {
                char::from_digit((random_numbers[i] & 0b1111) as u32, 16).expect(ERROR_MAKE_CHAR)
            }
            UuidElements::Random89AB => {
                char::from_digit((random_numbers[i] & 0b11) as u32 + 8, 16).expect(ERROR_MAKE_CHAR)
            }
            UuidElements::Version => '4',
            UuidElements::Hyphen => '-',
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use regex::Regex;

    #[test]
    fn test_uuid() {
        let uuid = uuid_from_numbers(&[0u8; 36]);
        assert_eq!(uuid, "00000000-0000-4000-8000-000000000000");
        let re =
            Regex::new(r"^[0-9:A-z]{8}-[0-9:A-z]{4}-4[0-9:A-z]{3}-[0-9:A-z]{4}-[0-9:A-z]{12}$")
                .unwrap();

        assert!(re.is_match(&uuid));
    }
}
