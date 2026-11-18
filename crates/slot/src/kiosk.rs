use std::collections::HashMap;

use slot_store::{Cart, Esrb, GameRule, KioskAuth, KioskConfig, Platform};
use slot_ui::{ButtonLegend, Legend, ListPage, ListRow};

use crate::scrape::EsrbView;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KioskPage {
    Main { cursor: usize },
    Systems { cursor: usize },
    GameSystems { cursor: usize },
    Games { platform: Platform, cursor: usize },
}

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum KioskMainRow {
    Toggle,
    Auth,
    Secret,
    EsrbCap,
    Systems,
    Games,
    ScrapeEsrb,
}

impl KioskMainRow {
    pub const ALL: [KioskMainRow; 7] = [
        KioskMainRow::Toggle,
        KioskMainRow::Auth,
        KioskMainRow::Secret,
        KioskMainRow::EsrbCap,
        KioskMainRow::Systems,
        KioskMainRow::Games,
        KioskMainRow::ScrapeEsrb,
    ];
}

pub fn valid_secret(auth: KioskAuth, secret: &str) -> Result<(), &'static str> {
    match auth {
        KioskAuth::Pin => {
            if (4..=12).contains(&secret.len()) && secret.bytes().all(|b| b.is_ascii_digit()) {
                Ok(())
            } else {
                Err("Use 4-12 digits")
            }
        }
        KioskAuth::Password => {
            let count = secret.chars().count();
            if (1..=64).contains(&count) {
                Ok(())
            } else {
                Err("Use 1-64 characters")
            }
        }
    }
}

pub fn systems_with_games(carts: &[Cart]) -> Vec<Platform> {
    Platform::ALL
        .into_iter()
        .filter(|p| *p != Platform::Apps && carts.iter().any(|c| c.platform == *p))
        .collect()
}

pub fn carts_on(carts: &[Cart], platform: Platform) -> Vec<&Cart> {
    carts.iter().filter(|c| c.platform == platform).collect()
}

pub fn main_page(cfg: &KioskConfig, esrb: &EsrbView, cursor: usize) -> ListPage {
    let secret_label = match cfg.auth {
        KioskAuth::Pin => "Set PIN",
        KioskAuth::Password => "Set Password",
    };
    let secret_detail = if cfg.secret.is_empty() {
        "Not Set"
    } else {
        "Set"
    };
    let sys_detail = if cfg.restrict_systems {
        format!("{} Selected", cfg.allowed_systems.len())
    } else {
        "All Systems".into()
    };
    let scrape_detail = if esrb.running {
        format!("{}/{}", esrb.done, esrb.total)
    } else if let Some(err) = &esrb.error {
        err.clone()
    } else if esrb.rated > 0 {
        format!("+{} rated", esrb.rated)
    } else {
        "Fetch Online".into()
    };
    let rows = vec![
        ListRow {
            label: "Enter Kiosk Mode".into(),
            detail: if cfg.secret.is_empty() {
                "Set Lock First".into()
            } else {
                "Lock Now".into()
            },
            bars: None,
            icon: None,
        },
        ListRow {
            label: "Lock Type".into(),
            detail: cfg.auth.label().into(),
            bars: None,
            icon: None,
        },
        ListRow {
            label: secret_label.into(),
            detail: secret_detail.into(),
            bars: None,
            icon: None,
        },
        ListRow {
            label: "ESRB Limit".into(),
            detail: cfg.esrb_cap.label().into(),
            bars: None,
            icon: None,
        },
        ListRow {
            label: "Allowed Systems".into(),
            detail: sys_detail,
            bars: None,
            icon: None,
        },
        ListRow {
            label: "Game Rules & ESRB".into(),
            detail: if cfg.whitelist_games {
                "Allowlist".into()
            } else {
                "Per Game".into()
            },
            bars: None,
            icon: None,
        },
        ListRow {
            label: "Fetch ESRB Ratings".into(),
            detail: scrape_detail,
            bars: None,
            icon: None,
        },
    ];
    let status = if cfg.esrb_cap != slot_store::EsrbCap::Off {
        "Unrated games are locked out until rated".to_string()
    } else {
        format!("Lock: {}", cfg.auth.label())
    };
    let n = rows.len();
    let cursor = cursor.min(n - 1);
    let selected = KioskMainRow::ALL[cursor];
    let mut legends = vec![Legend::new(ButtonLegend::B, "Back")];
    match selected {
        KioskMainRow::Auth | KioskMainRow::EsrbCap => {
            legends.push(Legend::new(ButtonLegend::Right, "Change"));
        }
        KioskMainRow::Systems => {
            legends.push(Legend::new(ButtonLegend::Right, "Toggle"));
            legends.push(Legend::new(ButtonLegend::A, "Systems"));
        }
        _ => {
            legends.push(Legend::new(ButtonLegend::A, "Select"));
        }
    }
    ListPage {
        title: "Kiosk Mode".into(),
        status,
        cursor,
        rows,
        legends,
    }
}

pub fn systems_page(cfg: &KioskConfig, cursor: usize) -> ListPage {
    let rows: Vec<ListRow> = Platform::ALL
        .into_iter()
        .map(|p| ListRow {
            label: p.name().into(),
            detail: if cfg.allowed_systems.contains(&p) {
                "Allowed".into()
            } else {
                "Hidden".into()
            },
            bars: None,
            icon: None,
        })
        .collect();
    let cursor = cursor.min(rows.len().saturating_sub(1));
    ListPage {
        title: "Allowed Systems".into(),
        status: if cfg.restrict_systems {
            format!(
                "{} of {} systems allowed",
                cfg.allowed_systems.len(),
                Platform::ALL.len()
            )
        } else {
            "Restricting systems is currently Off (All allowed)".into()
        },
        cursor,
        rows,
        legends: vec![
            Legend::new(ButtonLegend::B, "Back"),
            Legend::new(ButtonLegend::A, "Toggle"),
            Legend::new(ButtonLegend::Y, "All / None"),
        ],
    }
}

pub fn game_systems_page(carts: &[Cart], cfg: &KioskConfig, cursor: usize) -> ListPage {
    let systems = systems_with_games(carts);
    let rows: Vec<ListRow> = systems
        .iter()
        .map(|&p| {
            let count = carts.iter().filter(|c| c.platform == p).count();
            ListRow {
                label: p.name().into(),
                detail: format!("{count} games"),
                bars: None,
                icon: None,
            }
        })
        .collect();
    let cursor = cursor.min(rows.len().saturating_sub(1));
    ListPage {
        title: "Game Rules & ESRB".into(),
        status: if cfg.whitelist_games {
            "Mode: Only Explicitly Allowed Games".into()
        } else {
            "Mode: Allowed Unless Blocked / Over ESRB".into()
        },
        cursor,
        rows,
        legends: vec![
            Legend::new(ButtonLegend::B, "Back"),
            Legend::new(ButtonLegend::A, "Games"),
            Legend::new(ButtonLegend::X, "Allowlist"),
        ],
    }
}

pub fn games_page(
    carts: &[Cart],
    cfg: &KioskConfig,
    esrb: &HashMap<(Platform, String), Esrb>,
    manual: &HashMap<(Platform, String), Esrb>,
    platform: Platform,
    cursor: usize,
) -> ListPage {
    let list = carts_on(carts, platform);
    let rows: Vec<ListRow> = list
        .iter()
        .map(|cart| {
            let key = (cart.platform, cart.stem.clone());
            let rule = cfg.game_rule(cart.platform, &cart.stem);
            let rating = esrb.get(&key).copied();
            let is_manual = manual.contains_key(&key);
            let rating_str = match rating {
                Some(r) if is_manual => format!("{}*", r.code()),
                Some(r) => r.code().into(),
                None => "Unrated".into(),
            };
            let rule_str = match rule {
                GameRule::Default => {
                    if cfg.esrb_cap.allows(rating) && !cfg.whitelist_games {
                        "Shown"
                    } else {
                        "Hidden"
                    }
                }
                GameRule::Allow => "Allowed",
                GameRule::Block => "Blocked",
            };
            ListRow {
                label: slot_ui::label_text(cart),
                detail: format!("{rule_str} · {rating_str}"),
                bars: None,
                icon: None,
            }
        })
        .collect();
    let cursor = cursor.min(rows.len().saturating_sub(1));
    ListPage {
        title: platform.name().into(),
        status: format!("ESRB Cap: {} (* = manual)", cfg.esrb_cap.label()),
        cursor,
        rows,
        legends: vec![
            Legend::new(ButtonLegend::B, "Back"),
            Legend::new(ButtonLegend::A, "Rule"),
            Legend::new(ButtonLegend::Right, "ESRB"),
            Legend::new(ButtonLegend::X, "Allowlist"),
        ],
    }
}
