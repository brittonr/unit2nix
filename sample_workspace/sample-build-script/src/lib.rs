/// Returns the value set by the build script.
pub fn generated_value() -> &'static str {
    env!("GENERATED_VALUE")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_script_sets_value() {
        assert_eq!(generated_value(), "built-by-unit2nix");
    }
}
