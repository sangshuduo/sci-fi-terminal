//! Unit tests for the config module.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use super::load::merge_tables;
use super::theme::MIN_CONTRAST;
use super::validate::is_safe_id;
use super::*;

const DOC_EXAMPLE: &str = r#"
schema_version = 1

[appearance]
theme = "graphite"
font_family = "monospace"
font_size = 14.0
line_height = 1.15
ligatures = false
ui_scale = 1.0
reduced_motion = true

[terminal]
scrollback_lines = 10000
scrollback_max_mib = 32
cursor_shape = "block"
cursor_blink = false
confirm_multiline_paste = true
copy_on_select = false

[profiles.default]
# Empty executable means discover the OS default shell.
executable = ""
args = []
login_shell = false

[layout]
preset = "focus"
restore = true
max_sessions = 8

[panels.metrics]
enabled = true
interval_ms = 1000

[effects]
preset = "off"
intensity = 0.15
max_fps = 30

[[keybindings]]
action = "session.new_tab"
context = "global"
keys = "Ctrl+Shift+T"

[[keybindings]]
action = "terminal.copy"
context = "terminal"
keys = "Ctrl+Shift+C"
"#;

const THEME_EXAMPLE: &str = r##"
schema_version = 1
id = "my-graphite"
name = "My Graphite"

[colors]
background = "#101820"
surface = "#1B2630"
foreground = "#E5EDF3"
muted = "#A6B4BF"
accent = "#77C7E8"
error = "#FF8C8C"
selection = "#345568"
cursor = "#E5EDF3"

[terminal]
foreground = "#E5EDF3"
background = "#101820"
ansi = ["#14212B", "#D96F78", "#83B98A", "#D9BC76",
        "#7DA5D8", "#BB91CE", "#76BFC5", "#C7D0D8",
        "#697B89", "#FF939C", "#A2D7AA", "#F5D998",
        "#A1C4F0", "#D8B0E8", "#9DDFE4", "#F3F7FA"]
"##;

fn temp_dir(tag: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir =
        std::env::temp_dir().join(format!("sft-config-test-{}-{tag}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

fn p() -> &'static Path {
    Path::new("test.toml")
}

fn invalid_fields(result: Result<Config, ConfigError>) -> Vec<String> {
    match result {
        Err(ConfigError::Invalid(diags)) => diags.into_iter().map(|d| d.field).collect(),
        other => panic!("expected Invalid, got {other:?}"),
    }
}

fn with_edit(section_line: &str) -> String {
    format!("schema_version = 1\n{section_line}\n")
}

#[test]
fn defaults_validate_clean() {
    let config = Config::default();
    assert!(validate(&config).is_empty());
    assert!(config.profiles.contains_key("default"));
    assert_eq!(config.schema_version, SCHEMA_VERSION);
}

#[test]
fn doc_example_parses_and_matches_defaults_plus_keybindings() {
    let config = parse_config(DOC_EXAMPLE, p()).expect("doc example parses");
    let expected = Config {
        keybindings: config.keybindings.clone(),
        ..Config::default()
    };
    assert_eq!(config, expected);
    assert_eq!(config.keybindings.len(), 2);
    assert_eq!(config.keybindings[1].keys, "Ctrl+Shift+C");
}

#[test]
fn missing_schema_version_rejected() {
    let err = parse_config("[appearance]\nfont_size = 12.0\n", p()).unwrap_err();
    assert!(err.to_string().contains("schema_version"), "{err}");
}

#[test]
fn unknown_field_rejected_with_name_and_location() {
    let err =
        parse_config("schema_version = 1\n[appearance]\nfont_szie = 12.0\n", p()).unwrap_err();
    let msg = err.to_string();
    assert!(matches!(err, ConfigError::Parse { .. }));
    assert!(msg.contains("font_szie"), "{msg}");
    assert!(msg.contains("line 3"), "{msg}");
}

#[test]
fn unsupported_effect_preset_rejected() {
    let text = with_edit("[effects]\npreset = \"crt-max\"");
    assert!(matches!(
        parse_config(&text, p()),
        Err(ConfigError::Parse { .. })
    ));
}

#[test]
fn newer_schema_rejected_read_only() {
    let err = parse_config("schema_version = 2\nfuture_thing = true\n", p()).unwrap_err();
    let ConfigError::Invalid(diags) = err else {
        panic!("expected Invalid");
    };
    assert_eq!(diags[0].field, "schema_version");
    assert!(diags[0].message.contains("newer unsupported schema"));
    assert!(diags[0].message.contains("read-only"));
    assert_eq!(diags[0].file.as_deref(), Some(p()));
}

#[test]
fn range_rules_rejected() {
    let cases = [
        ("[appearance]\nfont_size = 7.5", "appearance.font_size"),
        ("[appearance]\nfont_size = 48.5", "appearance.font_size"),
        ("[appearance]\nfont_size = nan", "appearance.font_size"),
        ("[appearance]\nline_height = 2.1", "appearance.line_height"),
        ("[appearance]\nline_height = 0.9", "appearance.line_height"),
        ("[appearance]\nui_scale = 0.5", "appearance.ui_scale"),
        ("[appearance]\nui_scale = inf", "appearance.ui_scale"),
        ("[appearance]\ntheme = \"../evil\"", "appearance.theme"),
        ("[effects]\nintensity = 1.5", "effects.intensity"),
        ("[effects]\nmax_fps = 0", "effects.max_fps"),
        ("[effects]\nmax_fps = 61", "effects.max_fps"),
        (
            "[panels.metrics]\ninterval_ms = 999",
            "panels.metrics.interval_ms",
        ),
        ("[layout]\nmax_sessions = 0", "layout.max_sessions"),
        ("[layout]\nmax_sessions = 9", "layout.max_sessions"),
        (
            "[terminal]\nscrollback_lines = 100001",
            "terminal.scrollback_lines",
        ),
        (
            "[terminal]\nscrollback_max_mib = 0",
            "terminal.scrollback_max_mib",
        ),
        (
            "[terminal]\nscrollback_max_mib = 257",
            "terminal.scrollback_max_mib",
        ),
        (
            "[profiles.\"Bad Id\"]\nexecutable = \"sh\"",
            "profiles.Bad Id",
        ),
        (
            "[profiles.x]\nexecutable = \"a\\u0000b\"",
            "profiles.x.executable",
        ),
        (
            "[[keybindings]]\naction = \"\"\ncontext = \"g\"\nkeys = \"A\"",
            "keybindings[0].action",
        ),
        (
            "[[keybindings]]\naction = \"a\"\ncontext = \"g\"\nkeys = \"Ctrl+\"",
            "keybindings[0].keys",
        ),
    ];
    for (snippet, field) in cases {
        let fields = invalid_fields(parse_config(&with_edit(snippet), p()));
        assert!(fields.iter().any(|f| f == field), "{snippet}: {fields:?}");
    }
}

#[test]
fn range_boundaries_accepted() {
    let text = with_edit(
        "[appearance]\nfont_size = 48.0\nline_height = 1.0\nui_scale = 0.75\n\
         [terminal]\nscrollback_lines = 0\nscrollback_max_mib = 256\n\
         [effects]\nintensity = 1.0\nmax_fps = 60\n[layout]\nmax_sessions = 1",
    );
    parse_config(&text, p()).expect("boundaries valid");
}

#[test]
fn diagnostic_display_includes_file_and_field() {
    let d = Diagnostic::new("appearance.font_size", "bad").with_file(PathBuf::from("c.toml"));
    assert_eq!(d.to_string(), "c.toml: appearance.font_size: bad");
    assert!(is_safe_id("high-contrast") && !is_safe_id("") && !is_safe_id("A"));
}

fn table(text: &str) -> toml::Table {
    toml::from_str(text).expect("valid toml")
}

#[test]
fn merge_tables_recurses_replaces_arrays_and_keys_bindings() {
    let base = table(
        "[appearance]\nfont_size = 12.0\ntheme = \"signal\"\n[profiles.default]\nargs = [\"-a\", \"-b\"]\n\
         [[keybindings]]\naction = \"x\"\ncontext = \"g\"\nkeys = \"A\"\n\
         [[keybindings]]\naction = \"y\"\ncontext = \"g\"\nkeys = \"B\"",
    );
    let overlay = table(
        "[appearance]\nfont_size = 16.0\n[profiles.default]\nargs = [\"-c\"]\n\
         [[keybindings]]\naction = \"x\"\ncontext = \"g\"\nkeys = \"C\"\n\
         [[keybindings]]\naction = \"x\"\ncontext = \"t\"\nkeys = \"D\"",
    );
    let merged = merge_tables(&base, &overlay);
    assert_eq!(merged["appearance"]["font_size"].as_float(), Some(16.0));
    assert_eq!(merged["appearance"]["theme"].as_str(), Some("signal"));
    assert_eq!(
        merged["profiles"]["default"]["args"]
            .as_array()
            .map(Vec::len),
        Some(1)
    );
    let keys: Vec<_> = merged["keybindings"]
        .as_array()
        .expect("array")
        .iter()
        .map(|b| b["keys"].as_str().unwrap_or_default().to_owned())
        .collect();
    assert_eq!(keys, ["C", "B", "D"]);
}

#[test]
fn load_effective_applies_precedence() {
    let paths = ConfigPaths::from_dir(temp_dir("precedence"));
    fs::write(
        &paths.config_file,
        "schema_version = 1\n[appearance]\nfont_size = 12.0\ntheme = \"signal\"\n",
    )
    .expect("write config");
    fs::write(
        &paths.overrides_file,
        "schema_version = 1\n[appearance]\nfont_size = 18.0\n",
    )
    .expect("write overrides");
    let loaded = load_effective(&paths, false);
    assert!(loaded.diagnostics.is_empty(), "{:?}", loaded.diagnostics);
    assert_eq!(loaded.config.appearance.font_size, 18.0);
    assert_eq!(loaded.config.appearance.theme, "signal");
    assert_eq!(loaded.sources.len(), 2);

    let safe = load_effective(&paths, true);
    assert_eq!(safe.config.appearance.font_size, 12.0);
    assert!(paths.overrides_file.exists(), "safe mode must not delete");
}

#[test]
fn load_effective_skips_broken_override_and_keeps_user_values() {
    let paths = ConfigPaths::from_dir(temp_dir("broken"));
    fs::write(
        &paths.config_file,
        "schema_version = 1\n[layout]\nmax_sessions = 4\n",
    )
    .expect("write config");
    let broken = "schema_version = 1\n[layout\nmax_sessions = 2\n";
    fs::write(&paths.overrides_file, broken).expect("write overrides");
    let loaded = load_effective(&paths, false);
    assert_eq!(loaded.config.layout.max_sessions, 4);
    assert_eq!(loaded.diagnostics.len(), 1);
    assert!(
        loaded.diagnostics[0]
            .to_string()
            .contains("ui-overrides.toml")
    );
    assert_eq!(
        fs::read_to_string(&paths.overrides_file).expect("read"),
        broken
    );
}

#[test]
fn load_effective_missing_files_yields_defaults() {
    let paths = ConfigPaths::from_dir(temp_dir("missing"));
    let loaded = load_effective(&paths, false);
    assert_eq!(loaded.config, Config::default());
    assert!(loaded.diagnostics.is_empty() && loaded.sources.is_empty());
    assert!(load_file(&paths.config_file).expect("ok").is_none());
}

#[test]
fn oversize_config_rejected() {
    let dir = temp_dir("oversize");
    let path = dir.join("config.toml");
    let big = format!(
        "schema_version = 1\n#{}\n",
        "x".repeat(MAX_CONFIG_BYTES as usize)
    );
    fs::write(&path, big).expect("write");
    assert!(matches!(
        load_file(&path),
        Err(ConfigError::TooLarge { .. })
    ));
}

#[test]
fn write_atomic_round_trip_and_backup() {
    let dir = temp_dir("atomic");
    let path = dir.join("nested").join("ui-overrides.toml");
    write_atomic(&path, "first").expect("first write");
    assert_eq!(fs::read_to_string(&path).expect("read"), "first");
    write_atomic(&path, "second").expect("second write");
    assert_eq!(fs::read_to_string(&path).expect("read"), "second");
    let bak = path.with_file_name("ui-overrides.toml.bak");
    assert_eq!(fs::read_to_string(bak).expect("bak"), "first");
    let leftovers = fs::read_dir(path.parent().expect("parent"))
        .expect("list")
        .filter(|e| {
            e.as_ref()
                .is_ok_and(|e| e.file_name().to_string_lossy().contains(".tmp-"))
        })
        .count();
    assert_eq!(leftovers, 0);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(&path).expect("meta").permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn save_overrides_round_trips() {
    let paths = ConfigPaths::from_dir(temp_dir("save"));
    let mut config = parse_config(DOC_EXAMPLE, p()).expect("parse");
    config.appearance.theme = "daylight".into();
    save_overrides(&paths, &config).expect("save");
    let reloaded = load_file(&paths.overrides_file)
        .expect("load")
        .expect("exists");
    assert_eq!(reloaded, config);

    config.appearance.font_size = 100.0;
    assert!(matches!(
        save_overrides(&paths, &config),
        Err(ConfigError::Invalid(_))
    ));
}

#[test]
fn platform_paths_use_expected_names() {
    if let Some(paths) = ConfigPaths::platform_default() {
        assert!(paths.config_dir.ends_with("sci-fi-terminal"));
        assert!(paths.overrides_file.ends_with("ui-overrides.toml"));
    }
}

// ----------------------------------------------------------------- themes

#[test]
fn theme_doc_example_parses() {
    let theme = parse_theme(THEME_EXAMPLE, p()).expect("theme parses");
    assert_eq!(theme.id, "my-graphite");
    assert!(!theme.builtin);
    assert_eq!(theme.colors.accent, Rgb::new(0x77, 0xC7, 0xE8));
    assert_eq!(theme.terminal.ansi[15], Rgb::new(0xF3, 0xF7, 0xFA));
    assert!(theme.contrast_warnings().is_empty());
}

#[test]
fn theme_missing_ansi_entry_rejected() {
    let text = THEME_EXAMPLE.replace("\"#9DDFE4\", ", "");
    let err = parse_theme(&text, p()).unwrap_err();
    let ConfigError::Invalid(diags) = err else {
        panic!("expected Invalid");
    };
    assert!(diags.iter().any(|d| d.field == "terminal.ansi"));
}

#[test]
fn theme_missing_token_bad_color_unknown_field_and_bad_id_rejected() {
    let missing = THEME_EXAMPLE.replace("muted = \"#A6B4BF\"\n", "");
    assert!(
        parse_theme(&missing, p())
            .unwrap_err()
            .to_string()
            .contains("muted")
    );
    let bad = THEME_EXAMPLE.replace("#77C7E8", "#77C7E");
    assert!(matches!(
        parse_theme(&bad, p()),
        Err(ConfigError::Invalid(_))
    ));
    let unknown = THEME_EXAMPLE.replace("[colors]\n", "[colors]\nshader = \"x\"\n");
    assert!(
        parse_theme(&unknown, p())
            .unwrap_err()
            .to_string()
            .contains("shader")
    );
    let bad_id = THEME_EXAMPLE.replace("my-graphite", "../x");
    assert!(matches!(
        parse_theme(&bad_id, p()),
        Err(ConfigError::Invalid(_))
    ));
}

#[test]
fn hex_parsing_and_contrast() {
    assert_eq!(Rgb::parse_hex("#0aFF10"), Some(Rgb::new(10, 255, 16)));
    for bad in ["0AFF10", "#0AFF1", "#0AFF100", "#GGGGGG", "#+1+1+1"] {
        assert_eq!(Rgb::parse_hex(bad), None, "{bad}");
    }
    let ratio = contrast_ratio(Rgb::new(0, 0, 0), Rgb::new(255, 255, 255));
    assert!((ratio - 21.0).abs() < 0.01);
}

#[test]
fn indexed_colors_cover_ansi_cube_and_grayscale() {
    let theme = &builtin_themes()[0];
    assert_eq!(theme.indexed(1), theme.terminal.ansi[1]);
    assert_eq!(theme.indexed(16), Rgb::new(0, 0, 0));
    assert_eq!(theme.indexed(196), Rgb::new(255, 0, 0));
    assert_eq!(theme.indexed(110), Rgb::new(135, 175, 215));
    assert_eq!(theme.indexed(231), Rgb::new(255, 255, 255));
    assert_eq!(theme.indexed(232), Rgb::new(8, 8, 8));
    assert_eq!(theme.indexed(255), Rgb::new(238, 238, 238));
}

#[test]
fn builtin_themes_meet_contrast() {
    let themes = builtin_themes();
    let ids: Vec<_> = themes.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, ["graphite", "daylight", "signal", "high-contrast"]);
    for theme in &themes {
        assert!(theme.builtin && is_safe_id(&theme.id));
        assert!(theme.contrast_warnings().is_empty(), "{}", theme.id);
        let min = if theme.id == "high-contrast" {
            7.0
        } else {
            MIN_CONTRAST
        };
        let c = &theme.colors;
        assert!(
            contrast_ratio(c.foreground, c.background) >= min,
            "{}",
            theme.id
        );
        assert!(contrast_ratio(theme.terminal.foreground, theme.terminal.background) >= min);
    }
    let hc = find_theme(&themes, "high-contrast").expect("hc");
    let c = &hc.colors;
    for fg in [c.foreground, c.muted, c.accent, c.error, c.cursor] {
        for bg in [c.background, c.surface] {
            assert!(contrast_ratio(fg, bg) >= 7.0, "{fg:?} on {bg:?}");
        }
    }
    assert!(contrast_ratio(c.foreground, c.selection) >= 7.0);
}

#[test]
fn user_theme_loader_skips_oversize_and_non_toml() {
    let dir = temp_dir("themes");
    fs::write(dir.join("mine.toml"), THEME_EXAMPLE).expect("write");
    fs::write(dir.join("notes.txt"), "ignored").expect("write");
    let big = format!(
        "{THEME_EXAMPLE}\n#{}",
        "x".repeat(MAX_CONFIG_BYTES as usize)
    );
    fs::write(dir.join("big.toml"), big).expect("write");
    let (themes, errors) = load_user_themes(&dir);
    assert_eq!(themes.len(), 1);
    assert_eq!(errors.len(), 1);
    assert!(matches!(errors[0], ConfigError::TooLarge { .. }));
    let (none, no_errors) = load_user_themes(&dir.join("absent"));
    assert!(none.is_empty() && no_errors.is_empty());
}

#[cfg(unix)]
#[test]
fn user_theme_loader_skips_symlinks() {
    let outside = temp_dir("outside");
    let target = outside.join("secret.toml");
    fs::write(&target, THEME_EXAMPLE).expect("write");
    let dir = temp_dir("themes-link");
    std::os::unix::fs::symlink(&target, dir.join("linked.toml")).expect("symlink");
    let (themes, errors) = load_user_themes(&dir);
    assert!(themes.is_empty() && errors.is_empty());
}
