use support_sla::internal::SlaPolicy;

pub fn evaluate_ticket(policy: &SlaPolicy) {
    support_sla::public::evaluate(policy);
}
