extern crate ping;

#[test]
fn basic() {
    ping::ping("127.0.0.1".parse().unwrap());
}
