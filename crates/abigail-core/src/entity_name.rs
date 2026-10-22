/// Validate the one canonical human-facing Entity name contract.
///
/// Names are stored exactly as reviewed, so callers must not trim or otherwise
/// normalize invalid input on the user's behalf.
pub fn validate_entity_name(name: &str) -> Result<(), String> {
    let character_count = name.chars().count();
    if name.trim() != name
        || !(1..=80).contains(&character_count)
        || name.chars().any(char::is_control)
    {
        return Err(
            "Entity name must be trimmed, contain 1 to 80 characters, and have no control characters."
                .into(),
        );
    }
    Ok(())
}
