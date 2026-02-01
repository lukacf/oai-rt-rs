use oai_rt_rs::Calls;

#[test]
fn calls_new_accepts_api_key() {
    let calls = Calls::new("sk-test");
    assert!(calls.is_ok());
}
