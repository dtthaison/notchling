use crate::vault_ops::VaultOps;
use notchling_engine::delegate::skill::SkillTool;
use notchling_engine::install::skills::SkillSpec;
use notchling_engine::vault::skill as vault_skill;
use std::path::Path;
use std::sync::Arc;

pub fn spec(binary: &Path, vault: Arc<VaultOps>) -> SkillSpec {
    let binary = binary.to_string_lossy().into_owned();
    SkillSpec {
        name: vault_skill::NAME.to_string(),
        tools: vec![SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity],
        text: Box::new(move |tool| {
            let tool = match tool {
                SkillTool::Claude => vault_skill::SkillTool::Claude,
                SkillTool::Codex => vault_skill::SkillTool::Codex,
                SkillTool::Antigravity => vault_skill::SkillTool::Antigravity,
            };
            vault_skill::content(&binary, &vault.skill_names(), tool)
        }),
    }
}
