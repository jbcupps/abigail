use abigail_core::vault::unlock::{HybridUnlockProvider, UnlockProvider};

#[test]
fn kek_remains_stable_across_five_provider_switches() {
    std::env::set_var(
        "ABIGAIL_VAULT_PASSPHRASE",
        "vault-stability-passphrase-sections-22-27",
    );

    let unlock = HybridUnlockProvider::new();
    let baseline = unlock.root_kek().expect("baseline KEK must load");

    // Clear the env fallback to prove session KEK stability does not depend on
    // repeated passphrase reads during provider/model switches.
    std::env::remove_var("ABIGAIL_VAULT_PASSPHRASE");

    let simulated_switches = ["google", "xai", "claude-cli", "google", "xai"];
    for provider in simulated_switches {
        let current = unlock
            .root_kek()
            .unwrap_or_else(|e| panic!("switch to {provider} failed to keep KEK stable: {e}"));
        assert_eq!(
            baseline, current,
            "KEK drifted after simulated provider switch to {provider}"
        );
    }
}
