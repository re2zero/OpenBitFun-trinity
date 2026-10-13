//! Stable subagent identity: the name and avatar a run shows everywhere.
//!
//! Ported from the web UI (`src/web-ui/src/flow_chat/subagent-identity/**`):
//! the same FNV-1a hash over the same seeded strings picks from the same
//! ordered avatar catalog and the same ordered name pool, so a session reads
//! the same on both surfaces. Nothing here is stored: identity is a pure
//! function of the seed, which keeps it stable across reloads and locales.

/// Avatar catalog as bundled with the shell (`robot-01` … `robot-20`).
const AVATAR_COUNT: u32 = 20;
/// The web pool's version tag; part of the hashed seed, never reorder.
const AVATAR_SEED_PREFIX: &str = "subagent-identity-v1:avatar:";
const NAME_SEED_PREFIX: &str = "subagent-names-v1:";

/// Name pool in the web UI's order (30 entries). The localized copy lives in
/// the locale files under `gpui-shell-subagent-name-<key>`.
pub(crate) const NAME_KEYS: [&str; 30] = [
    "bouncy-bean",
    "cheeky-bao",
    "sneaky-mochi",
    "zooming-potato",
    "curious-otter",
    "disco-penguin",
    "noodle-ninja",
    "bubble-bandit",
    "sleepy-taco",
    "pixel-rascal",
    "jelly-rocket",
    "wobbly-pudding",
    "waffle-wizard",
    "sneezing-dragon",
    "giggle-fox",
    "turbo-snail",
    "marshmallow-pirate",
    "pogo-panda",
    "toast-detective",
    "zigzag-duck",
    "mango-goblin",
    "biscuit-bandit",
    "comet-hamster",
    "captain-ducky",
    "hiccup-seal",
    "popcorn-imp",
    "wobble-corgi",
    "ninja-peach",
    "cloud-hopper",
    "moon-bunny",
];

/// English pool, for the pre-locale fallback text.
pub(crate) const ENGLISH_NAMES: [&str; 30] = [
    "Bouncy Bean",
    "Cheeky Bao",
    "Sneaky Mochi",
    "Zooming Potato",
    "Curious Otter",
    "Disco Penguin",
    "Noodle Ninja",
    "Bubble Bandit",
    "Sleepy Taco",
    "Pixel Rascal",
    "Jelly Rocket",
    "Wobbly Pudding",
    "Waffle Wizard",
    "Sneezing Dragon",
    "Giggle Fox",
    "Turbo Snail",
    "Marshmallow Pirate",
    "Pogo Panda",
    "Toast Detective",
    "Zigzag Duck",
    "Mango Goblin",
    "Biscuit Bandit",
    "Comet Hamster",
    "Captain Ducky",
    "Hiccup Seal",
    "Popcorn Imp",
    "Wobble Corgi",
    "Ninja Peach",
    "Cloud Hopper",
    "Moon Bunny",
];

/// FNV-1a over the UTF-16-free byte string; the web client hashes code units,
/// which matches for the ASCII seeds used here.
fn hash(value: &str) -> u32 {
    let mut hash: u32 = 0x811c_9dc5;
    for byte in value.as_bytes() {
        hash ^= u32::from(*byte);
        hash = hash.wrapping_mul(0x0100_0193);
    }
    hash
}

/// Avatar asset path for a seed (an agent alias or the child session id).
pub(crate) fn avatar_path(seed: &str) -> String {
    let seed = seed.trim();
    if seed.is_empty() {
        return "subagent-avatars/robot-01.svg".to_string();
    }
    let index = hash(&format!("{AVATAR_SEED_PREFIX}{seed}")) % AVATAR_COUNT + 1;
    format!("subagent-avatars/robot-{index:02}.svg")
}

/// Index into [`NAME_KEYS`] (and the localized pool) for a seed.
pub(crate) fn name_index(seed: &str) -> usize {
    let seed = seed.trim();
    if seed.is_empty() {
        return 0;
    }
    (hash(&format!("{NAME_SEED_PREFIX}{seed}")) % NAME_KEYS.len() as u32) as usize
}

/// The localized display name for a seed, from the shell's loaded copy.
pub(crate) fn display_name(seed: &str, text: &crate::i18n::ShellText) -> String {
    let index = name_index(seed);
    text.subagent_names
        .get(index)
        .cloned()
        .unwrap_or_else(|| ENGLISH_NAMES[index].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_is_stable_and_seed_dependent() {
        assert_eq!(avatar_path("parser-review"), avatar_path("parser-review"));
        assert!(avatar_path("parser-review").starts_with("subagent-avatars/robot-"));
        assert_ne!(name_index("parser-review"), usize::MAX);
        // Empty seeds fall back to the catalog head instead of panicking.
        assert_eq!(avatar_path("  "), "subagent-avatars/robot-01.svg");
        assert_eq!(name_index(""), 0);
    }

    #[test]
    fn pools_match_the_web_catalog_size() {
        assert_eq!(NAME_KEYS.len(), 30);
        assert_eq!(ENGLISH_NAMES.len(), NAME_KEYS.len());
    }

    /// Cross-surface parity: the expected values come from the web resolver
    /// (`resolveSubagentAvatarId` / `resolveSubagentNameKey`) run on the same
    /// child session ids, so one run keeps one identity on both surfaces.
    /// The fixtures are the three subagent runs of session
    /// `3261d9fc-…` (the layout-parity smoke session): zigzag-duck,
    /// popcorn-imp and disco-penguin.
    #[test]
    fn hash_matches_the_web_resolver_for_known_seeds() {
        let expected: &[(&str, &str, usize)] = &[
            (
                "22a361f7-b41b-4afb-8e03-0b4a9e54408d",
                "subagent-avatars/robot-15.svg",
                19,
            ),
            (
                "0ea023ee-9e67-4300-9e89-3e9586438292",
                "subagent-avatars/robot-03.svg",
                25,
            ),
            (
                "72828ee3-dabe-4044-bbc1-85fd9c7d658e",
                "subagent-avatars/robot-15.svg",
                5,
            ),
        ];
        for (seed, avatar, name) in expected {
            assert_eq!(&avatar_path(seed), avatar, "avatar for {seed}");
            assert_eq!(name_index(seed), *name, "name for {seed}");
        }
        assert_eq!(NAME_KEYS[19], "zigzag-duck");
        assert_eq!(NAME_KEYS[25], "popcorn-imp");
        assert_eq!(NAME_KEYS[5], "disco-penguin");
    }
}
