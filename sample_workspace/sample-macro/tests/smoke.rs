use sample_macro::HelloMacro;

#[derive(HelloMacro)]
struct TestStruct;

#[test]
fn derive_produces_hello_method() {
    assert_eq!(TestStruct::hello(), "Hello from TestStruct!");
}
