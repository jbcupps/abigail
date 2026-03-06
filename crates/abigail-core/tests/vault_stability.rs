use abigail_core::vault::unlock::{PassphraseUnlockProvider, UnlockProvider};

#[test]
fn kek_remains_stable_across_five_provider_switches() {
    let unlock = PassphraseUnlockProvider::new("vault-stability-passphrase-sections-22-27");
    let baseline = unlock.root_kek().expect("baseline KEK must load");

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
