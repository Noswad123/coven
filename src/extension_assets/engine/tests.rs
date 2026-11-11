mod pi {
    use crate::pi_extension::unix::POLICY;
    include!("policy_tests.rs");
}
mod opencode {
    use crate::opencode_extension::POLICY;
    include!("policy_tests.rs");
}
