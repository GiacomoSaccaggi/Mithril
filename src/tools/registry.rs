#![allow(dead_code)]
use std::collections::HashMap;
use serde_json::{json, Value};

#[derive(Debug, Clone)]
pub struct ToolParam {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub success: bool,
    pub output: String,
}

impl ToolResult {
    pub fn ok(output: impl Into<String>) -> Self {
        Self { success: true, output: output.into() }
    }
    pub fn err(output: impl Into<String>) -> Self {
        Self { success: false, output: output.into() }
    }
}

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn parameters(&self) -> Vec<ToolParam>;
    fn execute(&self, args: &HashMap<String, String>) -> ToolResult;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register<T: Tool + 'static>(&mut self, tool: T) {
        self.tools.insert(tool.name().to_string(), Box::new(tool));
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn all(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    /// Port of ToolRegistry.toJsonSchema() — compact format injected into planner prompts.
    pub fn to_json_schema(&self) -> String {
        let tools: Vec<Value> = self.tools.values().map(|tool| {
            let params: Value = tool.parameters().iter().fold(
                serde_json::Map::new(),
                |mut map, p| {
                    map.insert(p.name.clone(), json!({
                        "type": p.param_type,
                        "required": p.required,
                        "description": p.description
                    }));
                    map
                },
            ).into();
            json!({
                "name": tool.name(),
                "description": tool.description(),
                "parameters": params
            })
        }).collect();

        serde_json::to_string_pretty(&tools).unwrap_or_default()
    }

    /// Port of ToolRegistry.toMcpToolList() — MCP-compliant tool list.
    pub fn to_mcp_tool_list(&self) -> Vec<Value> {
        self.tools.values().map(|tool| {
            let properties: Value = tool.parameters().iter().fold(
                serde_json::Map::new(),
                |mut map, p| {
                    map.insert(p.name.clone(), json!({
                        "type": p.param_type,
                        "description": p.description
                    }));
                    map
                },
            ).into();

            let params = tool.parameters();
            let required: Vec<&str> = params
                .iter()
                .filter(|p| p.required)
                .map(|p| p.name.as_str())
                .collect();

            let mut input_schema = json!({
                "type": "object",
                "properties": properties
            });
            if !required.is_empty() {
                input_schema["required"] = json!(required);
            }

            json!({
                "name": tool.name(),
                "description": tool.description(),
                "inputSchema": input_schema
            })
        }).collect()
    }

    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DummyTool;
    impl Tool for DummyTool {
        fn name(&self) -> &'static str { "dummy" }
        fn description(&self) -> &'static str { "A test tool" }
        fn parameters(&self) -> Vec<ToolParam> {
            vec![ToolParam {
                name: "input".into(),
                param_type: "string".into(),
                description: "test input".into(),
                required: true,
            }]
        }
        fn execute(&self, _: &HashMap<String, String>) -> ToolResult {
            ToolResult::ok("done")
        }
    }

    #[test]
    fn test_registry_register_get() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        assert!(reg.get("dummy").is_some());
        assert!(reg.get("nonexistent").is_none());
    }

    #[test]
    fn test_to_json_schema_valid() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        let schema = reg.to_json_schema();
        let parsed: Value = serde_json::from_str(&schema).unwrap();
        assert!(parsed.is_array());
        assert_eq!(parsed.as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_to_mcp_tool_list() {
        let mut reg = ToolRegistry::new();
        reg.register(DummyTool);
        let list = reg.to_mcp_tool_list();
        assert_eq!(list.len(), 1);
        assert!(list[0]["inputSchema"].is_object());
        assert_eq!(list[0]["inputSchema"]["required"][0], "input");
    }
}
