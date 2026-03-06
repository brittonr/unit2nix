use sample_lib::{Greeting, to_json};
use sample_macro::HelloMacro;

#[derive(HelloMacro)]
struct App;

fn main() {
    // Test library + serde feature
    let greeting = Greeting::new("unit2nix");
    println!("{}", to_json(&greeting));

    // Test proc-macro derive
    println!("{}", App::hello());

    // Test build script env var
    println!("build-script says: {}", sample_build_script::generated_value());
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn greeting_integration() {
        let g = Greeting::new("test");
        assert_eq!(g.message, "Hello, test!");
    }
}
