//! Where the player's files go in shared storage once Android lets the app use it (phones and
//! the Quest alike): a folder beside Downloads that any file manager, and a computer over USB,
//! opens.

/// The folder's name for an app id, as StarlingActivity.sharedFolder (Java) chooses it: a
/// development or preview build keeps its own beside the release's.
pub fn name(package: &str) -> &'static str {
    if package.ends_with(".dev") {
        "StarlingDev"
    } else if package.ends_with(".preview") {
        "StarlingPreview"
    } else {
        "Starling"
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn builds_keep_their_own_folders() {
        assert_eq!(super::name("io.github.juanluci.starling"), "Starling");
        assert_eq!(super::name("dev.horizon.quest"), "Starling");
        assert_eq!(
            super::name("io.github.juanluci.starling.dev"),
            "StarlingDev"
        );
        assert_eq!(
            super::name("io.github.juanluci.starling.preview"),
            "StarlingPreview"
        );
    }

    /// The Quest's files card reads these keys (game/files_offer.rs `quest_key`): each in both
    /// languages, and the unavailable card names the folder.
    #[test]
    fn the_headset_files_card_is_translated() {
        let english: serde_json::Value =
            serde_json::from_str(include_str!("../assets/horizon/lang/en_us.json")).unwrap();
        let russian: serde_json::Value =
            serde_json::from_str(include_str!("../assets/horizon/lang/ru_ru.json")).unwrap();
        for stage in ["ask", "waiting", "restart", "unavailable"] {
            for part in ["title", "body", "primary", "other"] {
                let key = format!("horizon.quest.files.{stage}.{part}");
                let en = english[&key].as_str().unwrap_or_else(|| panic!("{key}"));
                let ru = russian[&key].as_str().unwrap_or_else(|| panic!("{key}"));
                assert!(!en.is_empty() && !ru.is_empty(), "{key}");
            }
        }
        for lang in [&english, &russian] {
            let body = lang["horizon.quest.files.unavailable.body"]
                .as_str()
                .unwrap();
            assert_eq!(body.matches("%s").count(), 1);
        }
    }
}
