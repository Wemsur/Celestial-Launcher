//! Reading a `.minecraft` version directory the way PCL2 does.
//!
//! A `.minecraft` library is a directory of version folders, each holding one
//! `<folder>.json` (and usually `<folder>.jar`). The JSON is *not* required to
//! be a plain Minecraft version manifest: over the years launchers have written
//! several incompatible shapes into that same slot, and an instance created by
//! one launcher has to be readable by all the others.
//!
//! PCL2 settled on a single resolution order that covers every shape it has met
//! (`ModMinecraft.vb`), and this module reproduces it.
//!
//! One deliberate difference: PCL2's last resort is a regex over the *folder*
//! name, which lets a directory called `NewPVP` load by accident. The launcher
//! then hands that string to Mojang's version manifest and fails with
//! "Invalid game version: NewPVP" — an error that reads like a broken instance
//! rather than an unreadable manifest. The folder name is therefore never used
//! as an answer here; when every manifest-derived step comes up empty the
//! version resolves to `None` and the caller skips it.
//!
//! The shapes in play:
//!
//! - **Plain manifest** — one `VersionInfo` for the whole version. Loaders are
//!   already merged in, so `mainClass` is the loader's entry point.
//! - **Inheriting** — a thin version carrying `inheritsFrom` (and usually
//!   `jar`), which names the parent holding the real manifest. LiteLoader and
//!   old OptiFine write these.
//! - **HMCL patches** — `patches: [...]`, a set of fragments sorted by
//!   `priority` and merged into one manifest at load time. `id: "game"` is the
//!   vanilla body, the rest are loader layers. Distinguishable from a plain
//!   manifest by having no top-level `time` key.

use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// PCL2's `VanillaName` pattern, used on a manifest's `id` and nowhere else.
const MC_VERSION_PATTERN: &str = r"(([1-9][0-9]w[0-9]{2}[a-g])|((1|[2-9][0-9])\.[0-9]+(\.[0-9]+)?(-(pre|rc|snapshot-?)[1-9]*| Pre-Release( [1-9])?)?))(_unobfuscated)?";

/// A `.minecraft` version directory, resolved.
pub struct ResolvedVersion {
    /// The manifest after patches have been flattened and parents merged in.
    /// This is what should be handed to the launcher: it is the single
    /// `VersionInfo` PCL2 would launch with.
    pub json: Value,
    /// The vanilla version number, or `None` when PCL2 would say "Unknown".
    pub vanilla_name: Option<String>,
    /// The loader the version's text indicates.
    pub loader: crate::state::ModLoader,
}

/// Read a version directory, reproducing PCL2's load path.
///
/// `dir` is the version directory (e.g. `versions/NewPVP`). `folder_name` is
/// its last component — PCL2 passes this around as the version's identity and
/// writes it back into a merged HMCL manifest's `id`.
pub fn resolve_version_dir(
    dir: &Path,
    folder_name: &str,
) -> Option<ResolvedVersion> {
    let json_path = version_json_path(dir, folder_name)?;
    let text = std::fs::read_to_string(&json_path).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;

    let mut merged = flatten_patches(json, folder_name);

    // Follow `inheritsFrom` to the end of the chain, tracking the *next* target
    // in a local the way PCL2 does rather than re-reading the merged object.
    //
    // PCL2's loop reads the child's `inheritsFrom` once, then after each merge
    // replaces it with the parent's own `InheritName`. Re-reading the merged
    // object instead would stop the walk one level early: each merge leaves the
    // immediate parent's name in `inheritsFrom` (the child's value overwrites
    // the parent's), so the grandparent would never be reached — and the root of
    // the chain is where the vanilla version and asset index usually live.
    let mut next = merged
        .get("inheritsFrom")
        .and_then(|v| v.as_str())
        .filter(|name| *name != folder_name)
        .map(String::from);
    let mut guard = 0;
    while let Some(parent_name) = next.take() {
        guard += 1;
        if guard > 16 || parent_name.is_empty() {
            break;
        }
        let Some(parent) = read_sibling_version(dir, &parent_name) else {
            break;
        };
        next = parent
            .get("inheritsFrom")
            .and_then(|v| v.as_str())
            .filter(|name| *name != parent_name)
            .map(String::from);
        merged = inherit_merge(parent, merged);
    }

    let vanilla_name = vanilla_name(&merged, dir, folder_name);
    // PCL2 classifies the loader from `If(JsonObject, JsonText).ToString` — the
    // *merged* object, which already holds the whole inheritance chain. Reading
    // the child's own file instead would call `1.21.1-forge-optifine` vanilla,
    // because the Forge marker lives in the parent it inherits from.
    let loader = loader_from_text(&merged.to_string());

    Some(ResolvedVersion {
        json: merged,
        vanilla_name,
        loader,
    })
}

/// Load a version directory's manifest as a launchable `VersionInfo`.
///
/// This is what the launcher needs: `serde_json::from_str` on the raw file would
/// reject every shape except a plain manifest, because daedalus's `VersionInfo`
/// requires `assetIndex`, `downloads` and `assets` — which an HMCL patch file
/// and an `inheritsFrom` layer both leave to their fragments or their parent.
///
/// The resolution is PCL2's, so the result is the same single manifest PCL2
/// launches with. Returns `None` when the directory holds no manifest at all,
/// which the caller treats as "download it instead".
pub fn resolve_version_info(dir: &Path) -> Option<Value> {
    let folder_name = dir
        .file_name()
        .map(|name| name.to_string_lossy().to_string())?;
    resolve_version_dir(dir, &folder_name).map(|resolved| resolved.json)
}

/// Which file in a version directory holds its manifest.
///
/// Mirrors PCL2's `GetJsonPath`: `<folder>.json` when it exists, else the first
/// `*.json` in the directory that has all three of `mainClass`, `type` and
/// `id`. The three-key test is what keeps `celestial.json`, `content_cache.json`
/// and a stray `usercache.json` out of the running — taking the first parseable
/// JSON instead can land on any of those.
fn version_json_path(dir: &Path, folder_name: &str) -> Option<PathBuf> {
    let expected = dir.join(format!("{folder_name}.json"));
    if expected.exists() {
        return Some(expected);
    }

    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(object) = value.as_object() else {
            continue;
        };
        if object.contains_key("mainClass")
            && object.contains_key("type")
            && object.contains_key("id")
        {
            return Some(path);
        }
    }

    None
}

/// Load the version directory named `name`, sitting beside `dir`.
///
/// Its patches are flattened too — a parent is itself a version file and may be
/// in HMCL shape.
fn read_sibling_version(dir: &Path, name: &str) -> Option<Value> {
    let parent_dir = dir.parent()?.join(name);
    let json_path = version_json_path(&parent_dir, name)?;
    let text = std::fs::read_to_string(&json_path).ok()?;
    let json: Value = serde_json::from_str(&text).ok()?;
    Some(flatten_patches(json, name))
}

/// Flatten an HMCL `patches` manifest into one object.
///
/// PCL2's test is "has `patches` **and** has no `time`" — a plain manifest can
/// carry `patches` too, and merging it would discard the version's own
/// `arguments` and `downloads`.
///
/// Fragments are merged in ascending `priority`, each overwriting the keys it
/// declares, so the highest-priority fragment wins per key. Afterwards the
/// merged object's `id` becomes the folder name and `inheritsFrom` is dropped,
/// as PCL2 does: a flattened manifest has no parent, it already holds
/// everything.
fn flatten_patches(json: Value, folder_name: &str) -> Value {
    let Some(object) = json.as_object() else {
        return json;
    };
    let patches = object
        .get("patches")
        .and_then(|v| v.as_array())
        .filter(|patches| !patches.is_empty());
    if patches.is_none() || object.contains_key("time") {
        return Value::Object(object.clone());
    }

    let mut sorted: Vec<&Value> = patches
        .into_iter()
        .flatten()
        .filter(|patch| patch.get("id").is_some())
        .collect();
    sorted.sort_by_key(|patch| {
        patch.get("priority").and_then(|v| v.as_f64()).unwrap_or(0.0) as i64
    });

    let mut merged = Map::new();
    for patch in sorted {
        if let Some(patch_object) = patch.as_object() {
            merge_into(&mut merged, patch_object);
        }
    }

    merged.insert("id".to_string(), Value::String(folder_name.to_string()));
    merged.remove("inheritsFrom");
    Value::Object(merged)
}

/// Merge a child version into its parent.
///
/// Mirrors PCL2's order exactly: `libraries` is concatenated child-first — when
/// two jars carry the same class, the JVM picks the first one `-cp` lists, so
/// the child's own libraries must precede the parent's — while every other key
/// is merged onto the parent with the child's values winning, and arrays
/// concatenating parent-first.
///
/// `inheritsFrom` is left as the merge produced it: the child's value overwrites
/// the parent's, which is what PCL2 leaves behind too, and the cascade's
/// `inheritsFrom` step reads it to name the version.
fn inherit_merge(parent: Value, child: Value) -> Value {
    let (Some(parent_object), Some(child_object)) =
        (parent.as_object(), child.as_object())
    else {
        return child;
    };

    let mut libraries: Vec<Value> = child_object
        .get("libraries")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    if let Some(parent_libraries) =
        parent_object.get("libraries").and_then(|v| v.as_array())
    {
        libraries.extend(parent_libraries.iter().cloned());
    }

    let mut merged = parent_object.clone();
    merge_into(&mut merged, child_object);
    if !libraries.is_empty() {
        merged.insert("libraries".to_string(), Value::Array(libraries));
    }

    Value::Object(merged)
}

/// Merge `source`'s keys into `target`, leaving keys `source` omits alone.
///
/// This is Newtonsoft's `Merge` as PCL2 calls it: objects merge key by key, and
/// **arrays concatenate** (`MergeArrayHandling.Concat` is the default). The array
/// rule is load-bearing rather than incidental — a Forge or OptiFine layer that
/// carries its own `arguments.game` would otherwise replace the vanilla
/// `--username`, `--assetIndex` and `--gameDir` entries wholesale, and the game
/// would fail to start with an argument list that looks complete.
///
/// `libraries` is the one key the caller overrides afterwards, because its order
/// matters and the merge cannot express it.
fn merge_into(target: &mut Map<String, Value>, source: &Map<String, Value>) {
    for (key, value) in source {
        match (target.get_mut(key), value) {
            (Some(Value::Object(existing)), Value::Object(incoming)) => {
                merge_into(existing, incoming);
            }
            (Some(Value::Array(existing)), Value::Array(incoming)) => {
                existing.extend(incoming.iter().cloned());
            }
            _ => {
                target.insert(key.clone(), value.clone());
            }
        }
    }
}

/// PCL2's `VanillaName` cascade, in its order.
///
/// Every branch is a signal from the manifest or one it inherits from; none of
/// them guesses from a name. The last is a regex over the manifest's own `id`,
/// which is still manifest data — the folder name is never consulted.
fn vanilla_name(json: &Value, dir: &Path, folder_name: &str) -> Option<String> {
    // 1. A release before 2013 has no usable version metadata; PCL2 labels these
    //    "Old" and shows them as legacy rather than resolving them.
    if let Some(release_time) = json.get("releaseTime").and_then(|v| v.as_str())
        && let Some(year) =
            release_time.get(..4).and_then(|y| y.parse::<i32>().ok())
        && (2000..2013).contains(&year)
    {
        return Some("Old".to_string());
    }

    // 2. An unreleased snapshot.
    if json.get("type").and_then(|v| v.as_str()) == Some("pending") {
        return Some("pending".to_string());
    }

    // 3. The marker PCL2 and this launcher both write when they flatten a
    //    loader into a vanilla manifest.
    if let Some(client_version) =
        json.get("clientVersion").and_then(|v| v.as_str())
        && !client_version.is_empty()
    {
        return Some(client_version.to_string());
    }

    // 4. HMCL patches: the `game` fragment is the vanilla body. Patches are
    //    normally merged away by `flatten_patches`; this fires only for a
    //    manifest that had `patches` *and* a `time` key, which was left intact.
    if let Some(patches) = json.get("patches").and_then(|v| v.as_array()) {
        for patch in patches {
            if patch.get("id").and_then(|v| v.as_str()) == Some("game")
                && let Some(version) = patch.get("version").and_then(|v| v.as_str())
            {
                return Some(version.to_string());
            }
        }
    }

    // 5. Forge and NeoForge pass the vanilla version as the argument following
    //    `--fml.mcVersion`.
    if let Some(arguments) = json
        .get("arguments")
        .and_then(|v| v.get("game"))
        .and_then(|v| v.as_array())
    {
        let mut after_marker = false;
        for argument in arguments.iter().filter_map(|v| v.as_str()) {
            if after_marker {
                return Some(argument.to_string());
            }
            after_marker = argument == "--fml.mcVersion";
        }
    }

    // 6. An inheriting version. LiteLoader names its vanilla version in `jar`,
    //    so that wins; otherwise the parent's name is the version.
    let inherits_from = json
        .get("inheritsFrom")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if !inherits_from.is_empty() {
        if let Some(jar) = json.get("jar").and_then(|v| v.as_str())
            && !jar.is_empty()
        {
            return Some(jar.to_string());
        }
        return Some(inherits_from.to_string());
    }

    // 7. The vanilla client download URL embeds the version id.
    if let Some(downloads) = json.get("downloads")
        && let Some(hit) = seek(
            &downloads.to_string(),
            r#"launcher\.mojang\.com/mc/game/([^"/]+)"#,
        )
    {
        return Some(hit);
    }

    // 8. Loader artifacts name the vanilla version they were built for.
    let libraries =
        json.get("libraries").map(|v| v.to_string()).unwrap_or_default();
    for pattern in [
        r"net\.minecraftforge:forge:([0-9]{1,2}\.[0-9.]+)",
        r"net\.minecraftforge:fmlloader:([0-9]{1,2}\.[0-9.]+)",
        r"optifine:OptiFine:([0-9]{1,2}\.[0-9.]+)",
        r#"(?:fabricmc|quiltmc):intermediary:([^"/]+)"#,
    ] {
        if let Some(hit) = seek(&libraries, pattern) {
            return Some(hit);
        }
    }

    // 9. A version naming the manifest it wants reused.
    if let Some(jar) = json.get("jar").and_then(|v| v.as_str())
        && !jar.is_empty()
    {
        return Some(jar.to_string());
    }

    // 10. The `name` inside the client jar's own `version.json`. Guarded by
    //     length because PCL2 notes that field is sometimes a hash.
    if let Some(name) = version_json_name_from_jar(json, dir, folder_name)
        && name.len() < 32
    {
        return Some(name);
    }

    // 11. A version id inside the manifest's own `id`.
    if let Some(hit) = seek(
        &json.get("id").and_then(|v| v.as_str()).unwrap_or_default(),
        MC_VERSION_PATTERN,
    ) {
        return Some(hit.replace("_unobfuscated", ""));
    }

    // 12. Any version number anywhere in the rest of the manifest. PCL2 drops
    //     `libraries` before searching — 162 library entries each carry a version
    //     of their own, and the first version-shaped string in them is the
    //     artifact's, not the game's. Nothing else is removed.
    if let Some(object) = json.as_object() {
        let mut without_libraries = object.clone();
        without_libraries.remove("libraries");
        if let Some(hit) =
            seek(&Value::Object(without_libraries).to_string(), MC_VERSION_PATTERN)
        {
            return Some(hit.replace("_unobfuscated", ""));
        }
    }

    // PCL2 marks this point `Reliable = False` and reports "Unknown". Returning
    // `None` lets the caller skip the version rather than try to resolve a
    // meaningless id.
    //
    // PCL2's last resort is a regex over the *folder* name. That is what makes a
    // directory called `NewPVP` loadable in PCL2 by accident and unloadable in
    // every launcher that then hands the name to Mojang's version manifest, so
    // it is deliberately not reproduced here.
    tracing::info!("无法确认 MC 版本号的版本：{folder_name}");
    None
}

/// The client jar's embedded `version.json` `name`, when it can be read.
///
/// Mirrors PCL2's `JsonVersion`. The jar is the one named by the manifest's
/// `jar` key, or `<folder>.jar` beside it.
fn version_json_name_from_jar(
    json: &Value,
    dir: &Path,
    folder_name: &str,
) -> Option<String> {
    let jar_name = json
        .get("jar")
        .and_then(|v| v.as_str())
        .filter(|name| !name.is_empty())
        .map(|name| format!("{name}.jar"))
        .unwrap_or_else(|| format!("{folder_name}.jar"));
    let jar_path = dir.join(jar_name);
    if !jar_path.exists() {
        return None;
    }

    let bytes = std::fs::read(&jar_path).ok()?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let mut entry = archive.by_name("version.json").ok()?;
    let mut data = Vec::new();
    std::io::Read::read_to_end(&mut entry, &mut data).ok()?;
    let inner: Value = serde_json::from_slice(&data).ok()?;
    inner.get("name").and_then(|v| v.as_str()).map(String::from)
}

/// The first capture of `pattern` in `text`, or `None`.
///
/// PCL2's `RegexSeek` returns the whole match; every use here wraps the wanted
/// value in exactly one capture group, so the capture is returned.
fn seek(text: &str, pattern: &str) -> Option<String> {
    let regex = regex::Regex::new(pattern).ok()?;
    regex
        .captures(text)
        .and_then(|captures| captures.get(1))
        .map(|hit| hit.as_str().to_string())
}

/// Which loader a version JSON's raw text indicates, in PCL2's order.
///
/// PCL2 searches the whole serialized JSON rather than the parsed fields: the
/// marker string is the same whether the loader appears in `libraries`, in a
/// nested processor argument, or in a comment.
///
/// The Forge test explicitly excludes `net.neoforge`, because a NeoForge install
/// carries the odd `net.minecraftforge` artifact too — 21.1.251 ships
/// `net.minecraftforge:srgutils:0.4.15` — and testing Forge without that guard
/// would call every NeoForge install Forge.
fn loader_from_text(text: &str) -> crate::state::ModLoader {
    use crate::state::ModLoader;

    if text.contains("net.fabricmc:fabric-loader")
        || text.contains("org.quiltmc:quilt-loader")
    {
        ModLoader::Fabric
    } else if text.contains("minecraftforge") && !text.contains("net.neoforge") {
        ModLoader::Forge
    } else if text.contains("net.neoforge") {
        ModLoader::NeoForge
    } else {
        // PCL2 also tracks LiteLoader as its own state, which this launcher has
        // no counterpart for. A LiteLoader version is a vanilla version with a
        // tweaker on top, so it reads as vanilla here.
        ModLoader::Vanilla
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_version(dir: &Path, folder: &str, json: serde_json::Value) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(
            dir.join(format!("{folder}.json")),
            serde_json::to_string_pretty(&json).unwrap(),
        )
        .unwrap();
    }

    /// PCL2's `clientVersion` marker is the first thing it looks for.
    #[test]
    fn client_version_wins() {
        let root = tempfile::tempdir().unwrap();
        write_version(
            root.path(),
            "1.21.1",
            serde_json::json!({
                "id": "1.21.1",
                "clientVersion": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
            }),
        );

        let resolved = resolve_version_dir(root.path(), "1.21.1").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("1.21.1"));
    }

    /// A patch manifest *without* a top-level `time` is merged, as PCL2 does.
    ///
    /// Only the fragments survive the merge — a top-level key the fragments do
    /// not declare is dropped, `clientVersion` included. That is PCL2's behavior
    /// too: its `Merge` starts from an empty object and folds the fragments in,
    /// so an HMCL file must carry its version inside a fragment (as `game`'s
    /// `version`) or leave itself intact by also having a `time` key.
    ///
    /// What merging buys is a launchable manifest: the highest-priority
    /// fragment's `mainClass` wins, and keys only the vanilla fragment declares
    /// — `assetIndex`, `assets` — are carried through.
    #[test]
    fn patches_without_time_are_merged() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("NewPVP");
        write_version(
            &dir,
            "NewPVP",
            serde_json::json!({
                "id": "NewHack",
                "root": true,
                "assets": "29",
                "patches": [
                    {
                        "id": "game",
                        "version": "1.21.11",
                        "priority": 0,
                        "mainClass": "net.minecraft.client.main.Main",
                        "assetIndex": { "id": "29" },
                        "assets": "29",
                        "libraries": [{ "name": "net.fabricmc:intermediary:1.21.11" }],
                    },
                    {
                        "id": "fabric",
                        "version": "0.18.4",
                        "priority": 30000,
                        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                        "libraries": [{ "name": "net.fabricmc:fabric-loader:0.18.4" }],
                    },
                ],
            }),
        );

        let resolved = resolve_version_dir(&dir, "NewPVP").unwrap();
        assert_eq!(resolved.loader, crate::state::ModLoader::Fabric);

        assert_eq!(
            resolved.json["mainClass"].as_str(),
            Some("net.fabricmc.loader.impl.launch.knot.KnotClient")
        );
        // The vanilla fragment's assetIndex and assets are kept — the fabric
        // layer declares neither and must not erase them.
        assert_eq!(resolved.json["assetIndex"]["id"], "29");
        assert_eq!(resolved.json["assets"], "29");
    }

    /// The same HMCL file that keeps its `time` is *not* merged, so its
    /// `game` fragment's `version` still answers. This is the real
    /// `NewPVP.json` shape, and the difference between the two outcomes is
    /// exactly PCL2's condition.
    #[test]
    fn patches_with_time_keep_their_game_version() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("NewPVP");
        write_version(
            &dir,
            "NewPVP",
            serde_json::json!({
                "id": "NewHack",
                "jar": "NewPVP",
                "root": true,
                "time": "2025-12-09T20:23:30+08:00",
                "assets": "29",
                "patches": [
                    {
                        "id": "game",
                        "version": "1.21.11",
                        "priority": 0,
                        "mainClass": "net.minecraft.client.main.Main",
                        "assetIndex": { "id": "29" },
                        "libraries": [{ "name": "net.fabricmc:fabric-loader:0.18.4" }],
                    },
                    {
                        "id": "fabric",
                        "version": "0.18.4",
                        "priority": 30000,
                        "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                    },
                ],
            }),
        );

        let resolved = resolve_version_dir(&dir, "NewPVP").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("1.21.11"));
        assert_eq!(resolved.loader, crate::state::ModLoader::Fabric);
    }

    /// The folder name is never consulted as an answer.
    ///
    /// A folder called `NewPVP` whose manifest names itself in `jar` does resolve
    /// — to `NewPVP` — because PCL2's `jar` step fires before its `id` regex and
    /// does not require an `inheritsFrom`. That is still manifest data: the file
    /// asked for a manifest by that name. What this pins down is that the
    /// *folder* is not used, which the same fixture without `jar` shows.
    #[test]
    fn folder_name_is_never_the_version() {
        let root = tempfile::tempdir().unwrap();

        let named = root.path().join("NewPVP");
        write_version(
            &named,
            "NewPVP",
            serde_json::json!({
                "id": "NewHack",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
                "assets": "29",
            }),
        );
        // Nothing in the manifest names a version, so nothing is reported —
        // rather than the folder's name.
        assert_eq!(
            resolve_version_dir(&named, "NewPVP").unwrap().vanilla_name,
            None
        );

        // A folder whose name happens to look like a version does not get it
        // from the name either: the manifest still has to say so.
        let looks_like = root.path().join("1.21.11");
        write_version(
            &looks_like,
            "1.21.11",
            serde_json::json!({
                "id": "NewHack",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
            }),
        );
        assert_eq!(
            resolve_version_dir(&looks_like, "1.21.11").unwrap().vanilla_name,
            None
        );
    }

    /// Forge versions carry the vanilla version after `--fml.mcVersion`.
    #[test]
    fn fml_mc_version_argument_resolves() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("26.1.2-Forge_64.1.2");
        write_version(
            &dir,
            "26.1.2-Forge_64.1.2",
            serde_json::json!({
                "id": "26.1.2-Forge_64.1.2",
                "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "type": "release",
                "arguments": { "game": ["--fml.mcVersion", "26.1.2", "--launchTarget", "forgeclient"] },
                "libraries": [{ "name": "net.minecraftforge:forge:26.1.2-64.0.8:universal" }],
            }),
        );

        let resolved =
            resolve_version_dir(&dir, "26.1.2-Forge_64.1.2").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("26.1.2"));
        assert_eq!(resolved.loader, crate::state::ModLoader::Forge);
    }

    /// A forge artifact alone is enough, for a version with no fml argument.
    #[test]
    fn forge_artifact_resolves() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("Forge 26.1.2");
        write_version(
            &dir,
            "Forge 26.1.2",
            serde_json::json!({
                "id": "Forge 26.1.2",
                "mainClass": "net.minecraftforge.bootstrap.ForgeBootstrap",
                "type": "release",
                "libraries": [
                    { "name": "net.minecraftforge:forge:26.1.2-64.0.8:client" },
                    { "name": "net.minecraftforge:JarJarFileSystems:0.4.2" },
                ],
            }),
        );

        let resolved = resolve_version_dir(&dir, "Forge 26.1.2").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("26.1.2"));
        assert_eq!(resolved.loader, crate::state::ModLoader::Forge);
    }

    /// An intermediary artifact names the vanilla version too.
    #[test]
    fn intermediary_artifact_resolves() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("fabric");
        write_version(
            &dir,
            "fabric",
            serde_json::json!({
                "id": "fabric安装器",
                "mainClass": "net.fabricmc.loader.impl.launch.knot.KnotClient",
                "type": "release",
                "libraries": [{ "name": "net.fabricmc:intermediary:26.2" }],
            }),
        );

        let resolved = resolve_version_dir(&dir, "fabric").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("26.2"));
    }

    /// `inheritsFrom` is followed, and the child's keys beat the parent's.
    #[test]
    fn inherits_from_is_followed() {
        let root = tempfile::tempdir().unwrap();
        // A parent is a sibling version directory, not a file beside the child.
        write_version(
            &root.path().join("1.21.1"),
            "1.21.1",
            serde_json::json!({
                "id": "1.21.1",
                "clientVersion": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
                "assetIndex": { "id": "29" },
                "arguments": {
                    "game": ["--username", "${auth_player_name}", "--assetIndex", "29"],
                },
                "libraries": [{ "name": "com.mojang:authlib:7.0.63" }],
            }),
        );
        write_version(
            &root.path().join("1.21.1-LiteLoader"),
            "1.21.1-LiteLoader",
            serde_json::json!({
                "id": "1.21.1-LiteLoader",
                "inheritsFrom": "1.21.1",
                "jar": "1.21.1",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "type": "release",
                "arguments": {
                    "game": ["--tweakClass", "com.mumfrey.liteloader.launch.LiteLoaderTweaker"],
                },
                "libraries": [{ "name": "com.mumfrey:liteloader:1.21.1" }],
            }),
        );

        let resolved = resolve_version_dir(
            &root.path().join("1.21.1-LiteLoader"),
            "1.21.1-LiteLoader",
        )
        .unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("1.21.1"));
        // Child's mainClass wins, parent's assetIndex survives, and the child's
        // libraries come before the parent's.
        assert_eq!(
            resolved.json["mainClass"].as_str(),
            Some("net.minecraft.launchwrapper.Launch")
        );
        assert_eq!(resolved.json["assetIndex"]["id"], "29");
        // Arrays concatenate rather than replace: the vanilla arguments that log
        // the player in and pick the asset index are kept, and the child's own
        // are appended after them — the parent's object is the base the child is
        // merged into, exactly as PCL2's `InheritInstance.JsonObject.Merge` does.
        let game_arguments: Vec<&str> = resolved.json["arguments"]["game"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|argument| argument.as_str())
            .collect();
        assert_eq!(
            game_arguments,
            [
                "--username",
                "${auth_player_name}",
                "--assetIndex",
                "29",
                "--tweakClass",
                "com.mumfrey.liteloader.launch.LiteLoaderTweaker",
            ]
        );
        let libraries = resolved.json["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 2);
        assert_eq!(libraries[0]["name"], "com.mumfrey:liteloader:1.21.1");
        assert_eq!(libraries[1]["name"], "com.mojang:authlib:7.0.63");
    }

    /// A version naming itself as its parent must terminate, not spin.
    ///
    /// The chain walk declines to follow a self-reference, so `inheritsFrom`
    /// survives into the cascade and answers with the parent name — PCL2 does
    /// the same. What this pins down is that the loop ends: without the guard the
    /// parent would be merged on every pass and its libraries would repeat.
    #[test]
    fn self_inheritance_terminates() {
        let root = tempfile::tempdir().unwrap();
        write_version(
            &root.path().join("loop"),
            "loop",
            serde_json::json!({
                "id": "loop",
                "inheritsFrom": "loop",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
                "libraries": [{ "name": "com.mojang:authlib:7.0.63" }],
            }),
        );

        let resolved = resolve_version_dir(&root.path().join("loop"), "loop").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("loop"));
        let libraries = resolved.json["libraries"].as_array().unwrap();
        assert_eq!(libraries.len(), 1);
    }

    /// A chain longer than one level is followed to its end.
    #[test]
    fn inheritance_chain_is_followed() {
        let root = tempfile::tempdir().unwrap();
        write_version(
            &root.path().join("1.21.1"),
            "1.21.1",
            serde_json::json!({
                "id": "1.21.1",
                "clientVersion": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
                "assetIndex": { "id": "29" },
                "libraries": [{ "name": "com.mojang:authlib:7.0.63" }],
            }),
        );
        write_version(
            &root.path().join("1.21.1-forge"),
            "1.21.1-forge",
            serde_json::json!({
                "id": "1.21.1-forge",
                "inheritsFrom": "1.21.1",
                "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "type": "release",
                "libraries": [{ "name": "net.minecraftforge:forge:1.21.1-52.0.0" }],
            }),
        );
        write_version(
            &root.path().join("1.21.1-forge-optifine"),
            "1.21.1-forge-optifine",
            serde_json::json!({
                "id": "1.21.1-forge-optifine",
                "inheritsFrom": "1.21.1-forge",
                "mainClass": "net.minecraft.launchwrapper.Launch",
                "type": "release",
                "libraries": [{ "name": "optifine:OptiFine:1.21.1-HD_U_J1" }],
            }),
        );

        let leaf = root.path().join("1.21.1-forge-optifine");
        let resolved = resolve_version_dir(&leaf, "1.21.1-forge-optifine").unwrap();
        // The root of the chain carries the vanilla version.
        assert_eq!(resolved.vanilla_name.as_deref(), Some("1.21.1"));
        // The leaf's mainClass wins over every level.
        assert_eq!(
            resolved.json["mainClass"].as_str(),
            Some("net.minecraft.launchwrapper.Launch")
        );
        // Each level's libraries, leaf-first, exactly once.
        let names: Vec<&str> = resolved.json["libraries"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|l| l["name"].as_str())
            .collect();
        assert_eq!(
            names,
            [
                "optifine:OptiFine:1.21.1-HD_U_J1",
                "net.minecraftforge:forge:1.21.1-52.0.0",
                "com.mojang:authlib:7.0.63",
            ]
        );
        // The farthest ancestor's assetIndex survives.
        assert_eq!(resolved.json["assetIndex"]["id"], "29");
        // The loader is read from the merged chain, so a leaf that names no
        // loader of its own still reports the Forge its parent brought in.
        assert_eq!(resolved.loader, crate::state::ModLoader::Forge);
    }

    /// A sidecar JSON must never be mistaken for the manifest.
    #[test]
    fn sidecars_are_not_the_manifest() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1.21.1");
        std::fs::create_dir_all(&dir).unwrap();
        // `usercache.json` sorts before the manifest alphabetically and is a
        // bare array, so a "first parseable JSON" rule would take it.
        std::fs::write(dir.join("usercache.json"), "[]").unwrap();
        std::fs::write(
            dir.join("celestial.json"),
            serde_json::json!({ "created": "2025-01-01T00:00:00Z" }).to_string(),
        )
        .unwrap();

        let resolved = resolve_version_dir(&dir, "1.21.1");
        assert!(resolved.is_none());
    }

    /// A version number anywhere in the manifest is enough, when the earlier
    /// steps found nothing.
    #[test]
    fn version_anywhere_in_the_manifest_resolves() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("my-pack");
        write_version(
            &dir,
            "my-pack",
            serde_json::json!({
                "id": "my-pack",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
                "javaVersion": { "component": "java-runtime-delta", "majorVersion": 21 },
                "assetIndex": { "id": "1.21.11" },
                "libraries": [{ "name": "com.mojang:authlib:7.0.63" }],
            }),
        );

        let resolved = resolve_version_dir(&dir, "my-pack").unwrap();
        // `libraries` is excluded from the search, so the asset index's version
        // is what answers — not authlib's.
        assert_eq!(resolved.vanilla_name.as_deref(), Some("1.21.11"));
    }

    /// A pre-2013 release reads as legacy, not as a version id.
    #[test]
    fn old_release_is_marked_old() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("b1.7.3");
        write_version(
            &dir,
            "b1.7.3",
            serde_json::json!({
                "id": "b1.7.3",
                "releaseTime": "2011-07-01T00:00:00+00:00",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
            }),
        );

        let resolved = resolve_version_dir(&dir, "b1.7.3").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("Old"));
    }

    /// An unreleased snapshot.
    #[test]
    fn pending_snapshot_is_marked_pending() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1.21.11-rc1");
        write_version(
            &dir,
            "1.21.11-rc1",
            serde_json::json!({
                "id": "1.21.11-rc1",
                "type": "pending",
                "mainClass": "net.minecraft.client.main.Main",
            }),
        );

        let resolved = resolve_version_dir(&dir, "1.21.11-rc1").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("pending"));
    }

    /// A NeoForge install carries the odd `net.minecraftforge` artifact too, so
    /// the Forge test has to exclude `net.neoforge` or every NeoForge install
    /// reads as Forge.
    #[test]
    fn neoforge_is_not_called_forge() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("26.2-NeoForge");
        write_version(
            &dir,
            "26.2-NeoForge",
            serde_json::json!({
                "id": "26.2-NeoForge",
                "clientVersion": "26.2",
                "mainClass": "cpw.mods.bootstraplauncher.BootstrapLauncher",
                "type": "release",
                "libraries": [
                    { "name": "net.minecraftforge:srgutils:0.4.15" },
                    { "name": "net.neoforged:neoform:26.2.0.67" },
                ],
            }),
        );

        let resolved = resolve_version_dir(&dir, "26.2-NeoForge").unwrap();
        assert_eq!(resolved.vanilla_name.as_deref(), Some("26.2"));
        assert_eq!(resolved.loader, crate::state::ModLoader::NeoForge);
    }

    /// Quilt is read through the same branch as Fabric.
    #[test]
    fn quilt_is_read_as_fabric() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("quilt");
        write_version(
            &dir,
            "quilt",
            serde_json::json!({
                "id": "quilt",
                "clientVersion": "1.21.1",
                "mainClass": "org.quiltmc.loader.impl.launch.knot.KnotClient",
                "type": "release",
                "libraries": [{ "name": "org.quiltmc:quilt-loader:0.23.1" }],
            }),
        );

        let resolved = resolve_version_dir(&dir, "quilt").unwrap();
        assert_eq!(resolved.loader, crate::state::ModLoader::Fabric);
    }

    /// A version with no loader marker is vanilla.
    #[test]
    fn no_marker_is_vanilla() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("1.21.1");
        write_version(
            &dir,
            "1.21.1",
            serde_json::json!({
                "id": "1.21.1",
                "clientVersion": "1.21.1",
                "mainClass": "net.minecraft.client.main.Main",
                "type": "release",
            }),
        );

        let resolved = resolve_version_dir(&dir, "1.21.1").unwrap();
        assert_eq!(resolved.loader, crate::state::ModLoader::Vanilla);
    }
}
