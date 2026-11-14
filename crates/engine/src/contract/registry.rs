use super::antigravity::AntigravityProvider;
use super::claude::ClaudeProvider;
use super::codex::CodexProvider;
use super::{Capability, Provider, Support, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub tool: Tool,
    pub capability: Capability,
    pub support: Support,
    pub wired: bool,
}

impl Cell {
    pub fn entry(&self) -> String {
        format!("{} {}", self.tool.name(), self.capability.id())
    }
}

pub struct Registry {
    providers: Vec<Box<dyn Provider>>,
}

impl Registry {
    pub fn standard() -> Registry {
        Registry {
            providers: vec![
                Box::new(ClaudeProvider),
                Box::new(CodexProvider),
                Box::new(AntigravityProvider),
            ],
        }
    }

    pub fn of(&self, tool: Tool) -> &dyn Provider {
        self.providers[tool.index()].as_ref()
    }

    pub fn support(&self, tool: Tool, c: Capability) -> Support {
        self.of(tool).support(c)
    }

    pub fn is_wired(&self, tool: Tool, c: Capability) -> bool {
        self.of(tool).wired().contains(&c)
    }

    pub fn cells(&self) -> Vec<Cell> {
        let mut out = Vec::with_capacity(Tool::ALL.len() * Capability::ALL.len());
        for tool in Tool::ALL {
            for capability in Capability::ALL {
                out.push(Cell {
                    tool,
                    capability,
                    support: self.support(tool, capability),
                    wired: self.is_wired(tool, capability),
                });
            }
        }
        out
    }
}
