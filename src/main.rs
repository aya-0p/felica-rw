use eframe::egui;
use encoding_rs::{BIG5, EUC_JP, Encoding, GB18030, ISO_2022_JP, SHIFT_JIS, UTF_8, WINDOWS_1252};
use felica::{
    Reader, ReaderPreference,
    felica_standard::{
        BlockListElement, FelicaDriver, FelicaStandard, KeyStore, SearchServiceCodeResult,
        ServiceAttribute, ServiceCode, ServiceKind,
    },
    open_reader,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Default)]
struct BlockRow {
    number: u16,
    bytes: Vec<u8>,
}

#[derive(Clone, Default)]
struct SystemServices {
    code: String,
    services: Vec<ServiceEntry>,
}

#[derive(Clone, Default)]
struct ServiceEntry {
    number: u16,
    scope: String,
    kind: String,
    access: String,
    variants: Vec<ServiceVariant>,
}

#[derive(Clone, Default)]
struct ServiceVariant {
    code: u16,
    description: String,
    requires_key: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum ThemeChoice {
    #[default]
    Light,
    Dark,
    System,
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
enum TextEncodingChoice {
    #[default]
    Utf8,
    ShiftJis,
    EucJp,
    Iso2022Jp,
    Utf16Le,
    Utf16Be,
    Gb18030,
    Big5,
    Windows1252,
}

impl TextEncodingChoice {
    fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "UTF-8",
            Self::ShiftJis => "Shift_JIS",
            Self::EucJp => "EUC-JP",
            Self::Iso2022Jp => "ISO-2022-JP",
            Self::Utf16Le => "UTF-16LE",
            Self::Utf16Be => "UTF-16BE",
            Self::Gb18030 => "GB18030",
            Self::Big5 => "Big5",
            Self::Windows1252 => "Windows-1252",
        }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
struct AppSettings {
    ui_zoom: f32,
    theme: ThemeChoice,
    text_encoding: TextEncodingChoice,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            ui_zoom: 1.35,
            theme: ThemeChoice::Light,
            text_encoding: TextEncodingChoice::Utf8,
        }
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|mut path| {
        path.push("felica-rw");
        path.push("settings.toml");
        path
    })
}

fn load_settings() -> AppSettings {
    let Some(path) = settings_path() else {
        return AppSettings::default();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| toml::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let path = settings_path().ok_or_else(|| "設定ディレクトリを取得できません。".to_string())?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("設定ディレクトリを作成できません: {error}"))?;
    }
    let text = toml::to_string_pretty(settings)
        .map_err(|error| format!("設定を生成できません: {error}"))?;
    std::fs::write(path, text).map_err(|error| format!("設定を保存できません: {error}"))
}

fn add_service_entry(services: &mut Vec<ServiceEntry>, scope: &str, service: ServiceCode) {
    let raw = service.raw();
    let number = service.number();
    let (kind, access) = match service.kind() {
        Some(ServiceKind::Random) => (
            "Random",
            match service.attribute() {
                Some(ServiceAttribute::RandomReadOnly) => "read-only",
                _ => "read/write",
            },
        ),
        Some(ServiceKind::Cyclic) => (
            "Cyclic",
            match service.attribute() {
                Some(ServiceAttribute::CyclicReadOnly) => "read-only",
                _ => "read/write",
            },
        ),
        Some(ServiceKind::Purse) => (
            "Purse",
            match service.attribute() {
                Some(ServiceAttribute::PurseDirect) => "direct",
                Some(ServiceAttribute::PurseCashback) => "cashback/decrement",
                Some(ServiceAttribute::PurseDecrement) => "decrement",
                Some(ServiceAttribute::PurseReadOnly) => "read-only",
                _ => "unknown",
            },
        ),
        None => ("Unknown", "unknown"),
    };
    let description = service
        .attributes_description()
        .unwrap_or_else(|| format!("属性 {:02X}", service.attributes()));

    if let Some(entry) = services.iter_mut().find(|entry| {
        entry.scope == scope
            && entry.number == number
            && entry.kind == kind
            && entry.access == access
    }) {
        if !entry.variants.iter().any(|variant| variant.code == raw) {
            entry.variants.push(ServiceVariant {
                code: raw,
                description,
                requires_key: service.requires_key(),
            });
        }
    } else {
        services.push(ServiceEntry {
            number,
            scope: scope.to_string(),
            kind: kind.to_string(),
            access: access.to_string(),
            variants: vec![ServiceVariant {
                code: raw,
                description,
                requires_key: service.requires_key(),
            }],
        });
    }
}

fn collect_service_entries<D: FelicaDriver + ?Sized>(
    card: &mut FelicaStandard<D>,
    services: &mut Vec<ServiceEntry>,
) {
    let mut areas = Vec::<(u16, u16)>::new();
    for index in 0..=4095u16 {
        match card.search_service_code(index) {
            Ok(Some(SearchServiceCodeResult::Area {
                area_code,
                end_service_code,
            })) => {
                if !areas.contains(&(area_code, end_service_code)) {
                    areas.push((area_code, end_service_code));
                }
            }
            Ok(None) | Err(_) => break,
            Ok(Some(SearchServiceCodeResult::Service(_))) => {}
        }
    }

    for index in 0..=4095u16 {
        match card.search_service_code(index) {
            Ok(Some(SearchServiceCodeResult::Service(service))) => {
                let raw = service.raw();
                let matching_area = areas
                    .iter()
                    .filter(|(area_code, end)| raw >= *area_code && raw <= *end)
                    .min_by_key(|(area_code, end)| end.checked_sub(*area_code).unwrap_or(u16::MAX))
                    .map(|(area_code, _)| format!("AREA:{area_code:04X}"))
                    .unwrap_or_else(|| "ROOT".to_string());
                add_service_entry(services, &matching_area, service);
            }
            Ok(None) | Err(_) => break,
            Ok(Some(SearchServiceCodeResult::Area { .. })) => {}
        }
    }
}

struct FelicaApp {
    reader: Option<Reader>,
    reader_status: String,
    idm: String,
    pmm: String,
    data_rate: String,
    systems: Vec<SystemServices>,
    selected_system: String,
    service_codes: Vec<ServiceEntry>,
    blocks: Vec<BlockRow>,
    selected_service: String,
    block_number: String,
    block_count: String,
    write_hex: String,
    key_file: String,
    write_method: usize,
    read_method: usize,
    ui_zoom: f32,
    theme_choice: ThemeChoice,
    text_encoding: TextEncodingChoice,
    message: String,
}

impl Default for FelicaApp {
    fn default() -> Self {
        let settings = load_settings();
        Self {
            reader: None,
            reader_status: "リーダー未接続".to_string(),
            idm: String::new(),
            pmm: String::new(),
            data_rate: String::new(),
            systems: Vec::new(),
            selected_system: String::new(),
            service_codes: Vec::new(),
            blocks: Vec::new(),
            selected_service: "000B".to_string(),
            block_number: "0".to_string(),
            block_count: "256".to_string(),
            write_hex: String::new(),
            key_file: String::new(),
            write_method: 0,
            read_method: 0,
            ui_zoom: settings.ui_zoom.clamp(1.0, 2.0),
            theme_choice: settings.theme,
            text_encoding: settings.text_encoding,
            message: String::new(),
        }
    }
}

impl FelicaApp {
    fn save_preferences(&mut self) {
        let settings = AppSettings {
            ui_zoom: self.ui_zoom,
            theme: self.theme_choice,
            text_encoding: self.text_encoding,
        };
        if let Err(error) = save_settings(&settings) {
            self.message = error;
        }
    }

    fn connect_reader(&mut self) {
        match open_reader(ReaderPreference::Auto) {
            Ok(reader) => {
                let vendor = reader.vendor_name().unwrap_or("不明");
                let product = reader.product_name().unwrap_or("不明");
                self.reader_status = format!("接続中: {vendor} / {product}");
                self.reader = Some(reader);
                self.message = "リーダーを接続しました。カードをかざしてください。".to_string();
            }
            Err(error) => {
                self.reader = None;
                self.reader_status = "リーダー未接続".to_string();
                self.message = format!("リーダーを開けません: {error}");
            }
        }
    }

    fn scan_card(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            self.message = "先にリーダーを接続してください。".to_string();
            return;
        };

        let (mut card, _) = match FelicaStandard::polling_multi(
            reader.driver_mut(),
            &["424F", "212F"],
            0xFFFF,
            0x00,
            0x00,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.message = format!("カードを検出できません: {error}");
                return;
            }
        };

        self.idm = hex_string(card.idm());
        self.pmm = hex_string(card.pmm());
        self.data_rate = card.bitrate().to_string();

        let system_codes = match card.request_system_code() {
            Ok(systems) => systems,
            Err(error) => {
                self.message = format!("システムコード取得エラー: {error}");
                return;
            }
        };
        drop(card);

        let mut systems = Vec::new();
        for system_code in system_codes {
            let (mut system_card, _) = match FelicaStandard::polling_multi(
                reader.driver_mut(),
                &["424F", "212F"],
                system_code,
                0x00,
                0x00,
            ) {
                Ok(result) => result,
                Err(_) => continue,
            };

            let mut services: Vec<ServiceEntry> = Vec::new();
            collect_service_entries(&mut system_card, &mut services);
            systems.push(SystemServices {
                code: format!("{system_code:04X}"),
                services,
            });
        }
        self.systems = systems;
        self.selected_system.clear();
        self.service_codes.clear();
        self.selected_service.clear();
        self.blocks.clear();
        if let Some(first) = self.systems.first().cloned() {
            self.selected_system = first.code.clone();
            self.service_codes = first.services;
            if let Some(first_entry) = self.service_codes.first().cloned() {
                if let Some(first_variant) = first_entry.variants.first() {
                    self.selected_service = format!("{:04X}", first_variant.code);
                }
                self.read_selected_service();
            }
        }
        if self.message.is_empty() {
            self.message = format!(
                "カードを検出しました。サービス {}件",
                self.systems
                    .iter()
                    .map(|system| system.services.len())
                    .sum::<usize>()
            );
        }
    }

    fn select_system(&mut self, code: &str) {
        self.selected_system = code.to_string();
        self.selected_service.clear();
        self.blocks.clear();
        self.service_codes = self
            .systems
            .iter()
            .find(|system| system.code == code)
            .map(|system| system.services.clone())
            .unwrap_or_default();
        if let Some(first_entry) = self.service_codes.first().cloned() {
            if let Some(first_variant) = first_entry.variants.first() {
                self.selected_service = format!("{:04X}", first_variant.code);
            }
            self.read_selected_service();
        } else {
            self.message = format!("システム {code} にサービスはありません。");
        }
    }

    fn read_selected_service(&mut self) {
        let Ok(service) = parse_u16_hex(&self.selected_service) else {
            self.message = "サービスコードが不正です。".to_string();
            return;
        };
        let system_code = parse_u16_hex(&self.selected_system).unwrap_or(0xFFFF);

        let Some(reader) = self.reader.as_mut() else {
            self.message = "先にリーダーを接続してください。".to_string();
            return;
        };
        let (mut card, _) = match FelicaStandard::polling_multi(
            reader.driver_mut(),
            &["424F", "212F"],
            system_code,
            0x00,
            0x00,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.message = format!("選択したシステムを検出できません: {error}");
                return;
            }
        };

        let count = match card.request_block_information_ex(&[service]) {
            Ok(result) if !result.assigned_block_counts.is_empty() => {
                let count = usize::from(result.assigned_block_counts[0]).min(256);
                if count == 0 { 256 } else { count }
            }
            Ok(_) => 256,
            Err(_) => match card.request_block_information(&[service]) {
                Ok(counts) if !counts.is_empty() => {
                    let count = usize::from(counts[0]).min(256);
                    if count == 0 { 256 } else { count }
                }
                Ok(_) => 256,
                Err(_) => 256,
            },
        };
        drop(card);

        self.block_number = "0".to_string();
        self.block_count = count.to_string();
        self.read_blocks();
    }

    fn read_blocks(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            self.message = "先にリーダーを接続してください。".to_string();
            return;
        };
        let Ok(service) = parse_u16_hex(&self.selected_service) else {
            self.message =
                "サービスコードは 16進数 4桁で指定してください（例: 000B）。".to_string();
            return;
        };
        let Ok(start) = self.block_number.parse::<u16>() else {
            self.message = "開始ブロック番号が不正です。".to_string();
            return;
        };
        let Ok(count) = self.block_count.parse::<usize>() else {
            self.message = "ブロック数が不正です。".to_string();
            return;
        };
        if count == 0 || count > 256 {
            self.message = "ブロック数は 1～256 の範囲で指定してください。".to_string();
            return;
        }
        let system_code = parse_u16_hex(&self.selected_system).unwrap_or(0xFFFF);

        let (mut card, _) = match FelicaStandard::polling_multi(
            reader.driver_mut(),
            &["424F", "212F"],
            system_code,
            0x00,
            0x00,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.message = format!("カードを検出できません: {error}");
                return;
            }
        };

        let services = [ServiceCode::new(service)];
        let mut rows = Vec::new();
        let selected_variant_requires_key = self
            .service_codes
            .iter()
            .flat_map(|entry| entry.variants.iter())
            .any(|variant| variant.code == service && variant.requires_key);
        let use_des =
            self.read_method == 2 || (self.read_method == 0 && selected_variant_requires_key);
        let use_aes = self.read_method == 3;

        let secure_ready = if use_des || use_aes {
            if self.key_file.trim().is_empty() {
                self.message = "鍵ファイルを指定してください。".to_string();
                false
            } else {
                match KeyStore::from_jsonl_path(self.key_file.trim()) {
                    Ok(load) => match load.store.resolve(system_code, card.idm()) {
                        Some(resolved) => {
                            card.authenticate_node(&resolved, &[], &services, None)
                                .is_ok()
                                || {
                                    self.message =
                                            "相互認証に失敗しました。鍵またはカードを確認してください。"
                                                .to_string();
                                    false
                                }
                        }
                        None => {
                            self.message =
                                "選択中のカードとシステムに対応する鍵がありません。".to_string();
                            false
                        }
                    },
                    Err(error) => {
                        self.message = format!("鍵ファイルを読み込めません: {error}");
                        false
                    }
                }
            }
        } else {
            true
        };

        if !secure_ready {
            self.blocks.clear();
            return;
        }

        for offset in 0..count {
            let number = match start.checked_add(offset as u16) {
                Some(number) => number,
                None => break,
            };
            let block = BlockListElement::new(number, 0, 0);
            let read_result = if use_aes {
                card.read_v2(&[block]).map_err(|error| error.to_string())
            } else if use_des {
                card.read(&[block]).map_err(|error| error.to_string())
            } else {
                card.read_without_encryption(&services, &[block])
                    .map_err(|error| error.to_string())
            };
            match read_result {
                Ok(mut result) => {
                    if let Some(data) = result.pop() {
                        rows.push(BlockRow {
                            number,
                            bytes: data.to_vec(),
                        });
                    }
                }
                Err(error) => {
                    let remaining = count - offset;
                    self.message = format!("{remaining} 件目以降の読取りに失敗: {error}");
                    break;
                }
            }
        }
        self.blocks = rows;
        if !self.message.contains("読取りに失敗") {
            self.message = format!("{} ブロックを読取りました。", self.blocks.len());
        }
    }

    fn write_block(&mut self) {
        let Some(reader) = self.reader.as_mut() else {
            self.message = "先にリーダーを接続してください。".to_string();
            return;
        };
        let Ok(service) = parse_u16_hex(&self.selected_service) else {
            self.message = "サービスコードが不正です。".to_string();
            return;
        };
        let Ok(block_number) = self.block_number.parse::<u16>() else {
            self.message = "ブロック番号が不正です。".to_string();
            return;
        };
        let Ok(data) = parse_hex_bytes(&self.write_hex) else {
            self.message = "データは 16進数32文字（16バイト）で指定してください。".to_string();
            return;
        };
        if data.len() != 16 {
            self.message = "書込みデータは必ず 16バイト必要です。".to_string();
            return;
        }
        let system_code = parse_u16_hex(&self.selected_system).unwrap_or(0xFFFF);

        let (mut card, _) = match FelicaStandard::polling_multi(
            reader.driver_mut(),
            &["424F", "212F"],
            system_code,
            0x00,
            0x00,
        ) {
            Ok(result) => result,
            Err(error) => {
                self.message = format!("カードを検出できません: {error}");
                return;
            }
        };
        let services = [ServiceCode::new(service)];
        let access_mode = match self.write_method {
            1 | 2 => 0x01,
            3 => 0x04,
            _ => 0x00,
        };
        let blocks = [BlockListElement::new(block_number, 0, access_mode)];

        let result = if self.write_method == 4 || self.write_method == 5 {
            if self.key_file.trim().is_empty() {
                Err("鍵ファイルを指定してください。".to_string())
            } else {
                match KeyStore::from_jsonl_path(self.key_file.trim()) {
                    Ok(load) => match load.store.resolve(system_code, card.idm()) {
                        Some(resolved) => {
                            match card.authenticate_node(&resolved, &[], &services, None) {
                                Ok(_) => {
                                    if self.write_method == 4 {
                                        card.write(&blocks, &data)
                                            .map_err(|error| error.to_string())
                                    } else {
                                        card.write_v2(&blocks, &data)
                                            .map_err(|error| error.to_string())
                                    }
                                }
                                Err(error) => Err(error.to_string()),
                            }
                        }
                        None => {
                            Err("選択中のカードとシステムに対応する鍵がありません。".to_string())
                        }
                    },
                    Err(error) => Err(format!("鍵ファイルを読み込めません: {error}")),
                }
            }
        } else {
            card.write_without_encryption(&services, &blocks, &data)
                .map_err(|error| error.to_string())
        };

        match result {
            Ok(()) => {
                self.message =
                    format!("サービス {service:04X} のブロック {block_number} に書込みました。",)
            }
            Err(error) => self.message = format!("書込みに失敗しました: {error}"),
        }
    }
}

impl eframe::App for FelicaApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_zoom_factor(self.ui_zoom);
        ctx.options_mut(|options| options.fallback_theme = egui::Theme::Light);
        ctx.set_theme(match self.theme_choice {
            ThemeChoice::Light => egui::ThemePreference::Light,
            ThemeChoice::Dark => egui::ThemePreference::Dark,
            ThemeChoice::System => egui::ThemePreference::System,
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("main_window_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    let available_width = ui.available_width();
                    if available_width < 720.0 {
                        ui.spacing_mut().item_spacing.x = 6.0;
                    }

                    ui.horizontal_wrapped(|ui| {
                        ui.heading("FeliCa Reader / Writer");
                    });

                    egui::CollapsingHeader::new("表示設定")
                        .default_open(false)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label("テーマ");
                                egui::ComboBox::from_id_salt("theme_choice")
                                    .selected_text(match self.theme_choice {
                                        ThemeChoice::Light => "ライト",
                                        ThemeChoice::Dark => "ダーク",
                                        ThemeChoice::System => "システム設定に従う",
                                    })
                                    .show_ui(ui, |ui| {
                                        let light = ui.selectable_value(
                                            &mut self.theme_choice,
                                            ThemeChoice::Light,
                                            "ライト",
                                        );
                                        let dark = ui.selectable_value(
                                            &mut self.theme_choice,
                                            ThemeChoice::Dark,
                                            "ダーク",
                                        );
                                        let system = ui.selectable_value(
                                            &mut self.theme_choice,
                                            ThemeChoice::System,
                                            "システム設定に従う",
                                        );
                                        if light.changed() || dark.changed() || system.changed() {
                                            self.save_preferences();
                                        }
                                    });

                                let zoom = ui.add(
                                    egui::Slider::new(&mut self.ui_zoom, 1.0..=2.0)
                                        .text("表示倍率")
                                        .fixed_decimals(2),
                                );
                                if zoom.changed() {
                                    self.save_preferences();
                                }
                                ui.label(format!("{:.0}%", self.ui_zoom * 100.0));
                            });
                        });

                    ui.horizontal_wrapped(|ui| {
                        if ui.button("リーダー接続").clicked() {
                            self.connect_reader();
                        }
                        if ui.button("カード検出 / 一覧").clicked() {
                            self.scan_card();
                        }
                        ui.label(&self.reader_status);
                    });

                    ui.separator();
                    ui.columns(2, |columns| {
                        columns[0].group(|ui| {
                            ui.label("カード情報");
                            egui::Grid::new("card_info").striped(true).show(ui, |ui| {
                                ui.label("IDm");
                                ui.label(&self.idm);
                                ui.end_row();
                                ui.label("PMm");
                                ui.label(&self.pmm);
                                ui.end_row();
                                ui.label("通信速度");
                                ui.label(&self.data_rate);
                                ui.end_row();
                            });
                            ui.label("システムコード:");
                            if self.systems.is_empty() {
                                ui.label("-");
                            } else {
                                let systems = self.systems.clone();
                                ui.horizontal_wrapped(|ui| {
                                    for system in systems {
                                        let selected = system.code == self.selected_system;
                                        if ui.selectable_label(selected, &system.code).clicked() {
                                            self.select_system(&system.code);
                                        }
                                    }
                                });
                            }
                        });
                        columns[1].group(|ui| {
                            ui.label("選択システムのサービスコード:");
                            if self.service_codes.is_empty() {
                                ui.label("-");
                            } else {
                                let service_entries = self.service_codes.clone();
                                ui.vertical(|ui| {
                                    for entry in service_entries {
                                        let selected = entry.variants.iter().any(|variant| {
                                            format!("{:04X}", variant.code) == self.selected_service
                                        });
                                        let variants = entry
                                            .variants
                                            .iter()
                                            .map(|variant| {
                                                format!(
                                                    "{:04X}: {}",
                                                    variant.code, variant.description
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                            .join(" / ");
                                        let scope_label = if entry.scope == "ROOT" {
                                            "ルート".to_string()
                                        } else {
                                            entry.scope.replace("AREA:", "エリア ")
                                        };
                                        let label = format!(
                                            "{} / サービス番号 {:04X} / {} / {}: {}",
                                            scope_label,
                                            entry.number,
                                            entry.kind,
                                            entry.access,
                                            variants
                                        );
                                        if ui.selectable_label(selected, &label).clicked()
                                            && let Some(first) = entry.variants.first()
                                        {
                                            self.selected_service = format!("{:04X}", first.code);
                                            self.read_selected_service();
                                        }
                                    }
                                });
                            }
                        });
                    });

                    ui.separator();
                    ui.heading("ブロック操作");
                    ui.horizontal_wrapped(|ui| {
                        ui.label("サービスコード");
                        ui.text_edit_singleline(&mut self.selected_service);
                        ui.label("開始ブロック");
                        ui.text_edit_singleline(&mut self.block_number);
                        ui.label("件数");
                        ui.text_edit_singleline(&mut self.block_count);
                        egui::ComboBox::from_id_salt("read_method")
                            .selected_text(match self.read_method {
                                1 => "認証不要",
                                2 => "DES認証",
                                3 => "AES認証",
                                _ => "自動",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.read_method, 0, "自動");
                                ui.selectable_value(&mut self.read_method, 1, "認証不要");
                                ui.selectable_value(&mut self.read_method, 2, "DES認証");
                                ui.selectable_value(&mut self.read_method, 3, "AES認証");
                            });
                        egui::ComboBox::from_id_salt("text_encoding")
                            .selected_text(self.text_encoding.label())
                            .show_ui(ui, |ui| {
                                let choices = [
                                    TextEncodingChoice::Utf8,
                                    TextEncodingChoice::ShiftJis,
                                    TextEncodingChoice::EucJp,
                                    TextEncodingChoice::Iso2022Jp,
                                    TextEncodingChoice::Utf16Le,
                                    TextEncodingChoice::Utf16Be,
                                    TextEncodingChoice::Gb18030,
                                    TextEncodingChoice::Big5,
                                    TextEncodingChoice::Windows1252,
                                ];
                                for choice in choices {
                                    if ui
                                        .selectable_value(
                                            &mut self.text_encoding,
                                            choice,
                                            choice.label(),
                                        )
                                        .changed()
                                    {
                                        self.save_preferences();
                                    }
                                }
                            });
                        if ui.button("読み取り").clicked() {
                            self.read_blocks();
                        }
                    });

                    egui::ScrollArea::vertical()
                        .id_salt("block_data_scroll")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.horizontal_top(|ui| {
                                ui.vertical(|ui| {
                                    egui::Grid::new("block_hex_rows")
                                        .striped(true)
                                        .min_col_width(24.0)
                                        .show(ui, |ui| {
                                            ui.strong("ブロック");
                                            for offset in 0..16 {
                                                ui.strong(format!("{offset:X}"));
                                            }
                                            ui.end_row();
                                            for row in &self.blocks {
                                                ui.label(format!("{:04X}", row.number));
                                                for (offset, byte) in row.bytes.iter().enumerate() {
                                                    if offset >= 16 {
                                                        break;
                                                    }
                                                    ui.monospace(format!("{byte:02X}"));
                                                }
                                                for _ in row.bytes.len()..16 {
                                                    ui.label("");
                                                }
                                                ui.end_row();
                                            }
                                        });
                                });

                                egui::Frame::new()
                                    .inner_margin(egui::Margin::symmetric(8, 4))
                                    .stroke(egui::Stroke::new(
                                        1.0_f32,
                                        ui.visuals().widgets.noninteractive.bg_stroke.color,
                                    ))
                                    .show(ui, |ui| {
                                        ui.vertical(|ui| {
                                            egui::Grid::new("block_text_rows")
                                                .striped(true)
                                                .min_col_width(24.0)
                                                .show(ui, |ui| {
                                                    for offset in 0..16 {
                                                        ui.strong(format!("{offset:X}"));
                                                    }
                                                    ui.end_row();
                                                    for row in &self.blocks {
                                                        let text_cells = text_cells(
                                                            &row.bytes,
                                                            self.text_encoding,
                                                        );
                                                        for cell in text_cells {
                                                            ui.label(cell.as_deref().unwrap_or(""));
                                                        }
                                                        ui.end_row();
                                                    }
                                                });
                                        });
                                    });
                            });
                        });

                    ui.separator();
                    ui.heading("書込み");
                    ui.horizontal_wrapped(|ui| {
                        ui.label("16バイト（32桁）の16進データ");
                        ui.text_edit_singleline(&mut self.write_hex);
                        egui::ComboBox::from_id_salt("write_method")
                            .selected_text(match self.write_method {
                                1 => "キャッシュバック",
                                2 => "減算",
                                3 => "鍵変更",
                                4 => "DES暗号",
                                5 => "AES暗号",
                                _ => "通常",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.write_method, 0, "通常");
                                ui.selectable_value(&mut self.write_method, 1, "キャッシュバック");
                                ui.selectable_value(&mut self.write_method, 2, "減算");
                                ui.selectable_value(&mut self.write_method, 3, "鍵変更");
                                ui.selectable_value(&mut self.write_method, 4, "DES暗号");
                                ui.selectable_value(&mut self.write_method, 5, "AES暗号");
                            });
                        ui.label("鍵JSONL");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.key_file)
                                .desired_width(260.0)
                                .hint_text("DES/AES鍵ファイル"),
                        );
                        if ui.button("書き込む").clicked() {
                            self.write_block();
                        }
                    });

                    ui.separator();
                    ui.label(&self.message);
                });
        });
    }
}

fn hex_string(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02X}")).collect()
}

fn encoding_encoding(encoding: TextEncodingChoice) -> &'static Encoding {
    match encoding {
        TextEncodingChoice::Utf8 => UTF_8,
        TextEncodingChoice::ShiftJis => SHIFT_JIS,
        TextEncodingChoice::EucJp => EUC_JP,
        TextEncodingChoice::Iso2022Jp => ISO_2022_JP,
        TextEncodingChoice::Gb18030 => GB18030,
        TextEncodingChoice::Big5 => BIG5,
        TextEncodingChoice::Windows1252 => WINDOWS_1252,
        TextEncodingChoice::Utf16Le | TextEncodingChoice::Utf16Be => UTF_8,
    }
}

fn text_cells(bytes: &[u8], encoding: TextEncodingChoice) -> [Option<String>; 16] {
    let mut cells: [Option<String>; 16] = std::array::from_fn(|_| None);
    let mut offset = 0usize;

    while offset < bytes.len().min(16) {
        match encoding {
            TextEncodingChoice::Utf8 => match std::str::from_utf8(&bytes[offset..]) {
                Ok(text) => {
                    if let Some(character) = text.chars().next() {
                        let consumed = character.len_utf8();
                        if offset + consumed <= 16 {
                            cells[offset] = Some(character.to_string());
                        }
                        offset += consumed;
                    } else {
                        break;
                    }
                }
                Err(error) => {
                    let consumed = error.error_len().unwrap_or(1).min(16 - offset);
                    cells[offset] = Some("\u{FFFD}".to_string());
                    offset += consumed;
                }
            },
            TextEncodingChoice::Utf16Le | TextEncodingChoice::Utf16Be => {
                if offset + 1 >= bytes.len().min(16) {
                    break;
                }
                let unit = if encoding == TextEncodingChoice::Utf16Be {
                    u16::from_be_bytes([bytes[offset], bytes[offset + 1]])
                } else {
                    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
                };
                if matches!(unit, 0xFEFF | 0xFFFE) {
                    offset += 2;
                    continue;
                }
                if offset + 2 <= 16 {
                    cells[offset] = Some(String::from_utf16_lossy(&[unit]));
                }
                offset += 2;
            }
            _ => {
                let encoding = encoding_encoding(encoding);
                let mut consumed = 1;
                let mut character = "\u{FFFD}".to_string();
                for length in 1..=bytes.len().saturating_sub(offset).min(16 - offset) {
                    let (decoded, _) =
                        encoding.decode_without_bom_handling(&bytes[offset..offset + length]);
                    if let Some(first) = decoded.chars().next() {
                        consumed = length;
                        character = first.to_string();
                        if !first.is_control() && first != '\u{FFFD}' {
                            break;
                        }
                    }
                }
                cells[offset] = Some(character);
                offset += consumed;
            }
        }
    }
    cells
}

fn parse_hex_bytes(text: &str) -> Result<Vec<u8>, ()> {
    let compact: String = text
        .chars()
        .filter(|character| !character.is_ascii_whitespace())
        .collect();
    if !compact.len().is_multiple_of(2) {
        return Err(());
    }
    (0..compact.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&compact[index..index + 2], 16).map_err(|_| ()))
        .collect()
}

fn parse_u16_hex(text: &str) -> Result<u16, ()> {
    u16::from_str_radix(text.trim(), 16).map_err(|_| ())
}

fn find_cjk_font() -> Option<Vec<u8>> {
    const BUNDLED_FONT: &[u8] = include_bytes!("../assets/NotoSansJP-Regular.ttf");
    if !BUNDLED_FONT.is_empty() {
        return Some(BUNDLED_FONT.to_vec());
    }

    let candidates = [
        "/usr/share/fonts/opentype/NotoSansCJKjp/NotoSansCJKjp-Regular.otf",
        "/usr/share/fonts/opentype/NotoSansCJKjp/NotoSansCJKjp-Medium.otf",
        "/usr/share/fonts/truetype/Noto_Serif_JP/static/NotoSerifJP-Medium.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc",
        "/usr/share/fonts/truetype/horai-umefont/ume-tgs4.ttf",
        "/usr/share/fonts/opentype/ipafont-gothic/ipagp.ttf",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/truetype/Noto_Sans_CJK/NotoSansCJK-Regular.ttc",
        "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
        "C:\\Windows\\Fonts\\YuGothM.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
        "C:\\Windows\\Fonts\\msyh.ttc",
    ];

    candidates.iter().find_map(|path| std::fs::read(path).ok())
}

fn configure_cjk_fonts(ctx: &egui::Context) {
    let Some(font_bytes) = find_cjk_font() else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk_fallback".to_string(),
        Arc::new(egui::FontData::from_owned(font_bytes)),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push("cjk_fallback".to_string());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .push("cjk_fallback".to_string());
    ctx.set_fonts(fonts);
}

fn main() -> eframe::Result {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([800.0, 560.0])
        .with_resizable(true);
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "FeliCa Reader / Writer",
        options,
        Box::new(|cc| {
            configure_cjk_fonts(&cc.egui_ctx);
            let app = FelicaApp::default();
            cc.egui_ctx.set_zoom_factor(app.ui_zoom);
            cc.egui_ctx.set_theme(match app.theme_choice {
                ThemeChoice::Light => egui::ThemePreference::Light,
                ThemeChoice::Dark => egui::ThemePreference::Dark,
                ThemeChoice::System => egui::ThemePreference::System,
            });
            cc.egui_ctx
                .options_mut(|options| options.fallback_theme = egui::Theme::Light);
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::{TextEncodingChoice, hex_string, parse_hex_bytes, parse_u16_hex, text_cells};

    #[test]
    fn formats_and_parses_hex() {
        assert_eq!(hex_string(&[0x00, 0xAB, 0xFF]), "00ABFF");
        assert_eq!(parse_hex_bytes("00 AB FF").unwrap(), vec![0, 0xAB, 0xFF]);
        assert_eq!(parse_u16_hex("000B").unwrap(), 0x000B);
    }

    #[test]
    fn decodes_shift_jis_and_utf16() {
        assert_eq!(
            {
                let cells = text_cells(&[0x93, 0xFA, 0x96, 0x7B], TextEncodingChoice::ShiftJis);
                format!(
                    "{}{}",
                    cells[0].as_deref().unwrap_or(""),
                    cells[2].as_deref().unwrap_or("")
                )
            },
            "日本"
        );
        assert_eq!(
            {
                let cells = text_cells(&[0x65, 0x00, 0x2C, 0x67], TextEncodingChoice::Utf16Le);
                format!(
                    "{}{}",
                    cells[0].as_deref().unwrap_or(""),
                    cells[2].as_deref().unwrap_or("")
                )
            },
            "e本"
        );
    }

    #[test]
    fn places_multibyte_text_at_start_offset() {
        let cells = text_cells("日本".as_bytes(), TextEncodingChoice::Utf8);
        assert_eq!(cells[0].as_deref(), Some("日"));
        assert_eq!(cells[3].as_deref(), Some("本"));
        assert!(cells[1].is_none());
        assert!(cells[2].is_none());
        assert!(cells[4].is_none());
    }
}
