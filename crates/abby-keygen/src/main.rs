#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use abby_core::{
    generate_external_keypair, sign_constitutional_documents, AppConfig, Keyring,
    templates::CONSTITUTIONAL_DOCS,
};
use base64::Engine as _;
use ed25519_dalek::SigningKey;
use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();

    // Resolve data directory
    let data_dir = directories::ProjectDirs::from("com", "abby", "Abby")
        .map(|d| d.data_local_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));

    let docs_dir = data_dir.join("docs");

    // Check if setup has already been completed
    let pubkey_path = data_dir.join("external_pubkey.bin");
    let sigs_exist = docs_dir.join("soul.md.sig").exists()
        && docs_dir.join("ethics.md.sig").exists()
        && docs_dir.join("instincts.md.sig").exists()
        && pubkey_path.exists();

    if sigs_exist {
        tracing::info!("Identity setup already complete, skipping keygen.");
        std::process::exit(0);
    }

    // Perform all crypto operations before opening the GUI
    let setup_result = run_setup(&data_dir, &docs_dir);

    match setup_result {
        Ok(result) => {
            let app = KeygenApp::new(result);
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title("Abby Identity Setup")
                    .with_inner_size([600.0, 520.0])
                    .with_resizable(false)
                    .with_always_on_top(),
                ..Default::default()
            };
            eframe::run_native(
                "Abby Identity Setup",
                options,
                Box::new(|_cc| Ok(Box::new(app))),
            )
        }
        Err(e) => {
            tracing::error!("Setup failed: {}", e);
            // Show error GUI
            let app = KeygenApp::error(e);
            let options = eframe::NativeOptions {
                viewport: egui::ViewportBuilder::default()
                    .with_title("Abby Identity Setup - Error")
                    .with_inner_size([500.0, 200.0])
                    .with_resizable(false),
                ..Default::default()
            };
            eframe::run_native(
                "Abby Identity Setup - Error",
                options,
                Box::new(|_cc| Ok(Box::new(app))),
            )
        }
    }
}

struct SetupResult {
    private_key_base64: String,
    public_key_path: String,
}

fn run_setup(data_dir: &PathBuf, docs_dir: &PathBuf) -> Result<SetupResult, String> {
    // 1. Create directories
    std::fs::create_dir_all(docs_dir).map_err(|e| format!("Failed to create docs dir: {}", e))?;

    // 2. Copy constitutional templates (idempotent)
    for (name, content) in CONSTITUTIONAL_DOCS {
        let doc_path = docs_dir.join(name);
        if !doc_path.exists() {
            std::fs::write(&doc_path, content)
                .map_err(|e| format!("Failed to write {}: {}", name, e))?;
        }
    }

    // 3. Generate internal keyring if not present
    let keys_file = data_dir.join("keys.bin");
    if !keys_file.exists() {
        let keyring = Keyring::generate(data_dir.clone())
            .map_err(|e| format!("Failed to generate keyring: {}", e))?;
        keyring
            .save()
            .map_err(|e| format!("Failed to save keyring: {}", e))?;
    }

    // 4. Generate external Ed25519 keypair
    let keypair_result = generate_external_keypair(data_dir)
        .map_err(|e| format!("Failed to generate external keypair: {}", e))?;

    // 5. Parse private key for signing
    let private_key_bytes = base64::engine::general_purpose::STANDARD
        .decode(&keypair_result.private_key_base64)
        .map_err(|e| format!("Failed to decode private key: {}", e))?;

    let key_bytes: [u8; 32] = private_key_bytes
        .as_slice()
        .try_into()
        .map_err(|_| "Invalid private key length".to_string())?;

    let signing_key = SigningKey::from_bytes(&key_bytes);

    // 6. Sign constitutional documents
    sign_constitutional_documents(&signing_key, docs_dir)
        .map_err(|e| format!("Failed to sign documents: {}", e))?;

    // 7. Write config.json with external_pubkey_path set (birth_complete stays false)
    let config_path = data_dir.join("config.json");
    let config = if config_path.exists() {
        let mut c = AppConfig::load(&config_path)
            .map_err(|e| format!("Failed to load config: {}", e))?;
        c.external_pubkey_path = Some(keypair_result.public_key_path.clone());
        c
    } else {
        let mut c = AppConfig::default_paths();
        c.external_pubkey_path = Some(keypair_result.public_key_path.clone());
        c
    };
    config
        .save(&config_path)
        .map_err(|e| format!("Failed to save config: {}", e))?;

    Ok(SetupResult {
        private_key_base64: keypair_result.private_key_base64,
        public_key_path: keypair_result.public_key_path.to_string_lossy().to_string(),
    })
}

struct KeygenApp {
    private_key: Option<String>,
    public_key_path: String,
    key_saved: bool,
    copied: bool,
    copied_timer: f64,
    error: Option<String>,
    exit_requested: bool,
}

impl KeygenApp {
    fn new(result: SetupResult) -> Self {
        Self {
            private_key: Some(result.private_key_base64),
            public_key_path: result.public_key_path,
            key_saved: false,
            copied: false,
            copied_timer: 0.0,
            error: None,
            exit_requested: false,
        }
    }

    fn error(msg: String) -> Self {
        Self {
            private_key: None,
            public_key_path: String::new(),
            key_saved: false,
            copied: false,
            copied_timer: 0.0,
            error: Some(msg),
            exit_requested: false,
        }
    }
}

impl eframe::App for KeygenApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.exit_requested {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Tick copy timer
        if self.copied {
            self.copied_timer -= ctx.input(|i| i.predicted_dt as f64);
            if self.copied_timer <= 0.0 {
                self.copied = false;
            }
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // Error state
            if let Some(err) = &self.error {
                ui.heading("Setup Error");
                ui.separator();
                ui.colored_label(egui::Color32::RED, err.clone());
                ui.separator();
                if ui.button("Close").clicked() {
                    std::process::exit(1);
                }
                return;
            }

            let private_key = match &self.private_key {
                Some(k) => k.clone(),
                None => return,
            };

            ui.spacing_mut().item_spacing.y = 8.0;

            // Security warning banner
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(80, 60, 0))
                .inner_margin(egui::Margin::same(10))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.colored_label(
                        egui::Color32::YELLOW,
                        egui::RichText::new("CRITICAL: SAVE YOUR PRIVATE KEY")
                            .strong()
                            .size(16.0),
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 220, 100),
                        "This is the ONLY time you will see this key. Abby does NOT store it.",
                    );
                });

            ui.add_space(4.0);

            // Private key display
            ui.label("Your Private Signing Key (Ed25519, Base64):");
            let mut key_text = private_key.clone();
            ui.add(
                egui::TextEdit::multiline(&mut key_text)
                    .desired_rows(2)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .interactive(false),
            );

            // Copy + Save buttons
            ui.horizontal(|ui| {
                let copy_label = if self.copied { "Copied!" } else { "Copy to Clipboard" };
                if ui.button(copy_label).clicked() {
                    ui.output_mut(|o| o.copied_text = private_key.clone());
                    self.copied = true;
                    self.copied_timer = 2.0;
                }

                if ui.button("Save to File...").clicked() {
                    if let Some(path) = rfd::FileDialog::new()
                        .set_title("Save Private Key")
                        .set_file_name("abby_private_key.txt")
                        .save_file()
                    {
                        if let Err(e) = std::fs::write(&path, &private_key) {
                            self.error = Some(format!("Failed to save key: {}", e));
                        }
                    }
                }
            });

            ui.add_space(4.0);

            // Public key path
            ui.label("Public key saved to:");
            let mut path_text = self.public_key_path.clone();
            ui.add(
                egui::TextEdit::singleline(&mut path_text)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .interactive(false),
            );

            ui.add_space(4.0);

            // Security warnings
            egui::Frame::none()
                .fill(egui::Color32::from_rgb(60, 20, 20))
                .inner_margin(egui::Margin::same(10))
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 100, 100),
                        egui::RichText::new("SECURITY WARNINGS").strong(),
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 180, 180),
                        "- This key proves you are Abby's legitimate mentor.",
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 180, 180),
                        "- Store it securely (password manager, encrypted drive).",
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 180, 180),
                        "- Never share this key with anyone or any service.",
                    );
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 180, 180),
                        "- If you lose this key: you cannot re-verify integrity after reinstall.",
                    );
                });

            ui.add_space(4.0);

            // Checkbox
            ui.checkbox(
                &mut self.key_saved,
                "I have saved my private key securely and understand I will not see it again.",
            );

            ui.add_space(4.0);

            // Continue button
            ui.add_enabled_ui(self.key_saved, |ui| {
                if ui
                    .button(egui::RichText::new("Continue").size(16.0).strong())
                    .clicked()
                {
                    self.exit_requested = true;
                }
            });
        });
    }
}
