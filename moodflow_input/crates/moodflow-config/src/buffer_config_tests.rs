use crate::config::InputBufferConfig;

#[test]
fn partial_table_preserves_defaults_and_clamps_threshold() {
    let mut config: InputBufferConfig = toml::from_str("pace = '2s'").unwrap();
    assert_eq!(config.hold_ms(), 1200);
    assert_eq!(config.pace_ms(), 2000);
    assert!(!config.close_after_send);
    for (raw, expected) in [
        (-1, 500),
        (0, 500),
        (500, 500),
        (1200, 1200),
        (3000, 3000),
        (9000, 3000),
    ] {
        config.hold_ms = raw;
        assert_eq!(config.hold_ms(), expected);
    }
}

#[test]
fn pace_values_match_the_registered_domain() {
    for (pace, milliseconds) in [
        ("off", 0),
        ("1s", 1000),
        ("2s", 2000),
        ("3s", 3000),
        ("5s", 5000),
    ] {
        crate::config_schema::validate("input.buffer.pace", &toml::Value::String(pace.into()))
            .unwrap();
        let config = InputBufferConfig {
            pace: pace.into(),
            ..Default::default()
        };
        assert_eq!(config.pace_ms(), milliseconds);
    }
    assert!(
        crate::config_schema::validate("input.buffer.pace", &toml::Value::String("4s".into()))
            .is_err()
    );
}
