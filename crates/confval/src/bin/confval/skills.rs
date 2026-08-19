//! The embedded skill catalog, the write-time renderer, and the frontmatter
//! reader.
//!
//! Every skill file is embedded with `include_str!` at compile time, so
//! `confval init` writes with no network access and the text comes from the
//! crate the user installed. The embedded text is a template with one
//! placeholder, `{{confval_version}}`, replaced at write time with the crate
//! version.

/// One file shipped inside a skill, embedded at compile time.
pub(crate) struct SkillFile {
    /// The path relative to the skills root, for example
    /// `confval-init/references/pipeline.md`. It is also the path written under
    /// `<base>/.claude/skills/`.
    pub(crate) relative_path: &'static str,
    /// The embedded file text, a template rendered by [`render`].
    pub(crate) template: &'static str,
}

/// One skill: a directory of files named for the skill. `SKILL.md` is its own
/// field, so a catalog entry without one cannot be built.
pub(crate) struct Skill {
    /// The skill name, which matches its directory and decides its invocation.
    pub(crate) name: &'static str,
    /// The skill's `SKILL.md` file.
    pub(crate) skill_md: SkillFile,
    /// The reference files shipped beside `SKILL.md`.
    pub(crate) references: &'static [SkillFile],
}

impl Skill {
    /// Every file the skill installs, `SKILL.md` first.
    pub(crate) fn files(&self) -> impl Iterator<Item = &SkillFile> {
        std::iter::once(&self.skill_md).chain(self.references.iter())
    }
}

/// The skills this binary installs.
pub(crate) const SKILLS: &[Skill] = &[
    Skill {
        name: "confval-init",
        skill_md: SkillFile {
            relative_path: "confval-init/SKILL.md",
            template: include_str!("../../../skills/confval-init/SKILL.md"),
        },
        references: &[
            SkillFile {
                relative_path: "confval-init/references/pipeline.md",
                template: include_str!("../../../skills/confval-init/references/pipeline.md"),
            },
            SkillFile {
                relative_path: "confval-init/references/frontends.md",
                template: include_str!("../../../skills/confval-init/references/frontends.md"),
            },
            SkillFile {
                relative_path: "confval-init/references/patterns.md",
                template: include_str!("../../../skills/confval-init/references/patterns.md"),
            },
        ],
    },
    Skill {
        name: "confval-add-block",
        skill_md: SkillFile {
            relative_path: "confval-add-block/SKILL.md",
            template: include_str!("../../../skills/confval-add-block/SKILL.md"),
        },
        references: &[],
    },
];

/// The rendered bytes this binary writes for one file.
///
/// Rendering replaces every `{{confval_version}}` with the crate version, so
/// the byte comparison in `plan` compares against this output. An upgraded
/// binary therefore reports an untouched older file as differing.
pub(crate) fn render(file: &SkillFile) -> String {
    file.template
        .replace("{{confval_version}}", env!("CARGO_PKG_VERSION"))
}

/// The `description` value from a flat frontmatter block, or `None` when the
/// block is absent, unterminated, or carries no `description` key.
///
/// The reader handles the flat `key: value` form and nothing else. Shipped
/// descriptions are unquoted and contain no colon followed by a space, so no
/// quoting rule is needed.
pub(crate) fn description(skill_md: &str) -> Option<&str> {
    frontmatter_field(skill_md, "description")
}

/// The value of a top-level frontmatter key, read from the flat `key: value`
/// form. Returns `None` when the block is absent, unterminated, the key is
/// missing, or its value is empty.
fn frontmatter_field<'a>(skill_md: &'a str, key: &str) -> Option<&'a str> {
    let block = frontmatter_block(skill_md)?;
    for line in block.lines() {
        // An indented line is a nested value, not a top-level key.
        if line.starts_with(char::is_whitespace) {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if name == key {
            let value = value.trim();
            return (!value.is_empty()).then_some(value);
        }
    }
    None
}

/// The text between the opening `---` and the closing `---`, or `None` when the
/// block is absent or unterminated.
fn frontmatter_block(skill_md: &str) -> Option<&str> {
    let rest = skill_md.strip_prefix("---\n")?;
    let end = rest.find("\n---")?;
    Some(&rest[..end])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    /// The confval API names the skills tell an agent to write, decided against
    /// the skill content rather than against the crate's export surface.
    ///
    /// This list is not the whole promised surface. Names such as
    /// `range_constraint!`, `check_located`, `SourceMap`, and `to_fields` are
    /// pinned instead by the compiled full-program fences in the reference
    /// files. Keep both mechanisms in mind when editing a skill.
    const SKILL_API_NAMES: &[&str] = &[
        "Report",
        "Located",
        "Span",
        "Spec",
        "Config",
        "Lower",
        "Validate",
        "ValidateNested",
        "narrow",
        "KeywordSet",
        "keyword_enum!",
    ];

    /// The feature names the skills tell an agent to enable.
    const SKILL_FEATURE_NAMES: &[&str] = &[
        "derive", "hcl", "toml", "kdl", "json", "yaml", "color", "serde", "layering",
    ];

    /// The six portable Agent Skills frontmatter fields.
    const PORTABLE_FIELDS: &[&str] = &[
        "name",
        "description",
        "license",
        "compatibility",
        "metadata",
        "allowed-tools",
    ];

    fn skills_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("skills")
    }

    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                walk(&path, root, out);
            } else {
                let rel = path.strip_prefix(root).unwrap().to_string_lossy();
                out.push(rel.replace('\\', "/"));
            }
        }
    }

    /// The concatenated contents of every fenced code block across all skill
    /// files. The membership guard searches fences rather than prose, so an API
    /// name cannot be satisfied by an English sentence.
    fn all_fence_text() -> String {
        let mut fences = String::new();
        for skill in SKILLS {
            for file in skill.files() {
                let mut in_fence = false;
                for line in file.template.lines() {
                    if line.starts_with("```") {
                        in_fence = !in_fence;
                        continue;
                    }
                    if in_fence {
                        fences.push_str(line);
                        fences.push('\n');
                    }
                }
            }
        }
        fences
    }

    fn valid_skill_name(name: &str) -> bool {
        if name.is_empty() || name.len() > 64 {
            return false;
        }
        if name.starts_with('-') || name.ends_with('-') || name.contains("--") {
            return false;
        }
        name.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    }

    // Guard: promised API names are pinned in a `use`. A rename or removal of
    // any of these breaks this import. The whole `use` is gated on `derive`,
    // because `Spec` and `Config` are exported only under it. A consequence is
    // that the nine always-available names are pinned by a compiling `use` only
    // when a `derive` run compiles this test. Without the gate, the
    // `no-default-features`, `serde`, and `layering` matrix rows would fail to
    // compile the bin's test harness, since none of them enable `derive`.
    #[cfg(feature = "derive")]
    #[test]
    #[allow(unused_imports)]
    fn skill_api_names_resolve_against_the_crate() {
        use confval::prelude::{
            Config, KeywordSet, Located, Lower, Report, Span, Spec, Validate, ValidateNested,
            keyword_enum, narrow,
        };
    }

    // Guard: each API name appears inside a fenced code block of at least one
    // skill file, as a whole token rather than a fragment of a larger
    // identifier, so `Config` is not satisfied by `ServerConfig`.
    #[test]
    fn every_api_name_appears_in_a_fence() {
        let fences = all_fence_text();
        for name in SKILL_API_NAMES {
            assert!(
                contains_token(&fences, name),
                "API name `{name}` appears in no skill code fence as a whole token"
            );
        }
    }

    /// Whether `name` occurs in `text` as a whole token, not as a run of
    /// characters inside a larger identifier. `Config` must not match inside
    /// `ServerConfig`. A name ending in `!`, such as `keyword_enum!`, carries
    /// its own trailing boundary.
    fn contains_token(text: &str, name: &str) -> bool {
        let is_ident = |c: char| c.is_alphanumeric() || c == '_';
        let mut from = 0;
        while let Some(rel) = text[from..].find(name) {
            let start = from + rel;
            let end = start + name.len();
            let before_ok = start == 0 || !is_ident(text[..start].chars().next_back().unwrap());
            let after_ok = end == text.len() || !is_ident(text[end..].chars().next().unwrap());
            if before_ok && after_ok {
                return true;
            }
            from = start + 1;
        }
        false
    }

    // Guard: feature names are pinned. Every feature the skills name appears in
    // the crate's `[features]` table.
    #[test]
    fn every_named_feature_exists_in_cargo_toml() {
        let manifest =
            fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
        let features = manifest
            .split_once("[features]")
            .expect("crate manifest has a [features] table")
            .1;
        let features = features.split("\n[").next().unwrap();
        for feature in SKILL_FEATURE_NAMES {
            let declared = features
                .lines()
                .any(|line| line.starts_with(&format!("{feature} =")));
            assert!(
                declared,
                "feature `{feature}` is not in the [features] table"
            );
        }
    }

    // Guard: the catalog matches the source directory. A reference file added to
    // the directory and forgotten in the catalog fails here.
    #[test]
    fn catalog_matches_the_source_directory() {
        let root = skills_root();
        let mut on_disk = Vec::new();
        walk(&root, &root, &mut on_disk);
        let on_disk: BTreeSet<String> = on_disk.into_iter().collect();

        let embedded: BTreeSet<String> = SKILLS
            .iter()
            .flat_map(|skill| skill.files())
            .map(|file| file.relative_path.to_string())
            .collect();

        assert_eq!(on_disk, embedded);
    }

    // Guard: body links resolve. Every `references/…` link in a `SKILL.md` body
    // names a file the catalog carries for that same skill.
    #[test]
    fn body_reference_links_resolve_in_the_catalog() {
        for skill in SKILLS {
            let body = skill.skill_md.template;
            let carried: BTreeSet<&str> = skill.files().map(|f| f.relative_path).collect();
            for link in reference_links(body) {
                let target = format!("{}/{link}", skill.name);
                assert!(
                    carried.contains(target.as_str()),
                    "{} links to {link}, which the catalog does not carry",
                    skill.name
                );
            }
        }
    }

    fn reference_links(body: &str) -> Vec<String> {
        let mut links = Vec::new();
        let mut rest = body;
        while let Some(start) = rest.find("references/") {
            let tail = &rest[start..];
            let end = tail
                .find(|c: char| c.is_whitespace() || c == ')' || c == ']')
                .unwrap_or(tail.len());
            links.push(tail[..end].to_string());
            rest = &tail[end..];
        }
        links
    }

    // Guard: the frontmatter obeys the specification.
    #[test]
    fn frontmatter_obeys_the_specification() {
        for skill in SKILLS {
            let body = skill.skill_md.template;
            let block = frontmatter_block(body).expect("SKILL.md has a frontmatter block");

            let name = frontmatter_field(body, "name").expect("frontmatter has a name");
            assert_eq!(name, skill.name, "name must match the directory");
            assert!(
                valid_skill_name(name),
                "name `{name}` breaks the character rule"
            );

            let description = description(body).expect("frontmatter has a description");
            assert!(!description.is_empty());
            assert!(description.len() <= 1024);
            assert!(
                !description.starts_with('"') && !description.starts_with('\''),
                "description must be unquoted"
            );
            assert!(
                !description.contains(": "),
                "description must contain no colon followed by a space"
            );
            assert!(
                description.starts_with(|c: char| c.is_ascii_alphabetic()),
                "description must start with a letter"
            );

            for line in block.lines() {
                if line.starts_with(char::is_whitespace) {
                    continue;
                }
                let Some((key, _)) = line.split_once(':') else {
                    continue;
                };
                assert!(
                    PORTABLE_FIELDS.contains(&key),
                    "frontmatter key `{key}` is outside the six portable fields"
                );
            }
        }
    }

    // Guard: each body is at or under 500 lines, the adopted recommendation.
    #[test]
    fn each_body_is_within_the_line_limit() {
        for skill in SKILLS {
            let lines = skill.skill_md.template.lines().count();
            assert!(lines <= 500, "{} body is {lines} lines", skill.name);
        }
    }

    // Guard: a rendered file carries no placeholder.
    #[test]
    fn a_rendered_file_carries_no_placeholder() {
        for skill in SKILLS {
            for file in skill.files() {
                assert!(
                    !render(file).contains("{{"),
                    "{} left a placeholder after rendering",
                    file.relative_path
                );
            }
        }
    }

    #[test]
    fn version_is_substituted_into_the_rendered_output() {
        let init = &SKILLS[0].skill_md;
        assert!(init.template.contains("{{confval_version}}"));
        assert!(render(init).contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn description_reads_a_well_formed_block() {
        let md = "---\nname: demo\ndescription: A short summary\n---\n\n# Demo\n";
        assert_eq!(description(md), Some("A short summary"));
    }

    #[test]
    fn description_of_an_absent_block_is_none() {
        let md = "# Demo\n\nNo frontmatter here.\n";
        assert_eq!(description(md), None);
    }

    #[test]
    fn description_of_an_unterminated_block_is_none() {
        let md = "---\nname: demo\ndescription: A short summary\n";
        assert_eq!(description(md), None);
    }

    #[test]
    fn description_absent_key_is_none() {
        let md = "---\nname: demo\n---\n\n# Demo\n";
        assert_eq!(description(md), None);
    }
}
