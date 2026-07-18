pub mod internal {
    pub struct SlaPolicy;
}
pub mod public {
    use super::internal::SlaPolicy;

    pub fn evaluate(_policy: &SlaPolicy) {}
}
