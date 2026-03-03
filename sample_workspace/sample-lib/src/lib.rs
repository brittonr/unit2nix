/// A greeting message.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Greeting {
    pub message: String,
}

impl Greeting {
    pub fn new(name: &str) -> Self {
        Self {
            message: format!("Hello, {name}!"),
        }
    }
}

/// Serialize a greeting to JSON (only available with `serde` feature).
#[cfg(feature = "serde")]
pub fn to_json(greeting: &Greeting) -> String {
    serde_json::to_string(greeting).expect("serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn greeting_works() {
        let g = Greeting::new("world");
        assert_eq!(g.message, "Hello, world!");
    }

    #[cfg(feature = "serde")]
    #[test]
    fn json_roundtrip() {
        let g = Greeting::new("nix");
        let json = to_json(&g);
        let parsed: Greeting = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.message, "Hello, nix!");
    }
}
