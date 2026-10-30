//! Port of `src/shared/utils/radix-matcher.ts`.
//!
//! Nodes live in an arena (`Vec<Node>` addressed by `usize`) so the TypeScript
//! parent-pointer pruning in `remove` maps over without `Rc<RefCell<_>>`.
//! Child collections stay insertion-ordered, matching JS `Map` iteration order,
//! because emit order and match order are observable.

use indexmap::IndexMap;
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Normal = 0,
    Placeholder = 1,
    Wildcard = 2,
}

pub fn infer_node_type(part: &str) -> NodeType {
    if part.starts_with("**") {
        return NodeType::Wildcard;
    }
    if part.starts_with(':') || part == "*" {
        return NodeType::Placeholder;
    }
    NodeType::Normal
}

#[derive(Debug)]
struct Node<D> {
    node_type: NodeType,
    parent: Option<usize>,
    data: Option<D>,
    normal: Vec<(String, usize)>,
    placeholder: Vec<(String, usize)>,
    wildcard: Option<usize>,
}

impl<D> Node<D> {
    fn new(node_type: NodeType, parent: Option<usize>) -> Self {
        Self {
            node_type,
            parent,
            data: None,
            normal: Vec::new(),
            placeholder: Vec::new(),
            wildcard: None,
        }
    }

    fn has_children(&self) -> bool {
        !self.normal.is_empty() || !self.placeholder.is_empty() || self.wildcard.is_some()
    }
}

#[derive(Clone, Debug)]
pub struct FindResult<D> {
    pub data: D,
    /// `None` models the JS `params: undefined` on static hits; the emitter
    /// hands `{}` to listeners only when the matcher produced params.
    pub params: Option<Value>,
}

pub struct RadixMatcher<D> {
    nodes: Vec<Node<D>>,
    static_routes: IndexMap<String, D>,
}

/// `/`-separated parts, dropping empties, like `route.split('/').filter(Boolean)`.
fn split_parts(route: &str) -> Vec<&str> {
    route.split('/').filter(|p| !p.is_empty()).collect()
}

impl<D: Clone> RadixMatcher<D> {
    pub fn new() -> Self {
        Self {
            nodes: vec![Node::new(NodeType::Normal, None)],
            static_routes: IndexMap::new(),
        }
    }

    pub fn validate_route(route: &str) -> anyhow::Result<()> {
        let parts = split_parts(route);

        if parts.is_empty() {
            anyhow::bail!("route should not be empty");
        }

        for (i, part) in parts.iter().enumerate() {
            if part.starts_with(':') {
                if part.chars().count() == 1 {
                    anyhow::bail!("placeholder should have a name");
                }
            } else if part.starts_with("**") {
                if part.chars().count() != 2 {
                    anyhow::bail!("wildcard should be **");
                }
                if i != parts.len() - 1 {
                    anyhow::bail!("wildcard should be the last part");
                }
            } else if part.starts_with('*') {
                if part.chars().count() != 1 {
                    anyhow::bail!("placeholder * should be the only part");
                }
            } else if part.contains(':') || part.contains('*') {
                anyhow::bail!("normal nodes should not have : or *");
            }
        }

        Ok(())
    }

    fn maybe_dynamic_route(route: &str) -> bool {
        route.contains('*') || route.contains(':')
    }

    fn push_node(&mut self, node_type: NodeType, parent: usize) -> usize {
        self.nodes.push(Node::new(node_type, Some(parent)));
        self.nodes.len() - 1
    }

    /// Walk a route spec down the trie; mirrors `_findDynamicRouteNode`.
    fn find_dynamic_route_node(&self, route: &str) -> Option<usize> {
        let parts = split_parts(route);
        let mut node = 0usize;
        let mut unnamed = 0usize;
        let mut i = 0usize;
        // `consumed != parts.len()` models JS `i !== parts.length` after a break
        let mut consumed = parts.len();

        while i < parts.len() {
            let part = parts[i];
            match infer_node_type(part) {
                NodeType::Normal => {
                    match self.nodes[node].normal.iter().find(|(k, _)| k == part) {
                        Some((_, child)) => node = *child,
                        None => {
                            consumed = i;
                            break;
                        }
                    }
                }
                NodeType::Placeholder => {
                    let owned;
                    let key = if part == "*" {
                        owned = format!("_{unnamed}");
                        unnamed += 1;
                        owned.as_str()
                    } else {
                        &part[1..]
                    };
                    match self.nodes[node].placeholder.iter().find(|(k, _)| k == key) {
                        Some((_, child)) => node = *child,
                        None => {
                            consumed = i;
                            break;
                        }
                    }
                }
                NodeType::Wildcard => {
                    if let Some(child) = self.nodes[node].wildcard {
                        node = child;
                    }
                    // JS breaks without advancing `i`, so `i !== parts.length` holds
                    consumed = i;
                    break;
                }
            }
            i += 1;
        }

        let broke_early = consumed != parts.len();
        let node_ref = &self.nodes[node];

        if (broke_early && node_ref.node_type != NodeType::Wildcard) || node_ref.data.is_none() {
            return None;
        }

        Some(node)
    }

    pub fn insert(&mut self, route: &str, data: D) -> anyhow::Result<()> {
        Self::validate_route(route)?;

        if !Self::maybe_dynamic_route(route) {
            if self.static_routes.contains_key(route) {
                anyhow::bail!("route '{}' already exists", route);
            }
            self.static_routes.insert(route.to_string(), data);
            return Ok(());
        }

        let parts = split_parts(route);
        let mut node = 0usize;
        let mut unnamed = 0usize;

        for part in parts.iter() {
            match infer_node_type(part) {
                NodeType::Normal => {
                    let existing = self.nodes[node].normal.iter().find(|(k, _)| k == part).map(|(_, c)| *c);
                    node = match existing {
                        Some(child) => child,
                        None => {
                            let child = self.push_node(NodeType::Normal, node);
                            self.nodes[node].normal.push((part.to_string(), child));
                            child
                        }
                    };
                }
                NodeType::Placeholder => {
                    let owned;
                    let key = if *part == "*" {
                        owned = format!("_{unnamed}");
                        unnamed += 1;
                        owned.as_str()
                    } else {
                        &part[1..]
                    };
                    let existing =
                        self.nodes[node].placeholder.iter().find(|(k, _)| k == key).map(|(_, c)| *c);
                    node = match existing {
                        Some(child) => child,
                        None => {
                            let child = self.push_node(NodeType::Placeholder, node);
                            self.nodes[node].placeholder.push((key.to_string(), child));
                            child
                        }
                    };
                }
                NodeType::Wildcard => {
                    node = match self.nodes[node].wildcard {
                        Some(child) => child,
                        None => {
                            let child = self.push_node(NodeType::Wildcard, node);
                            self.nodes[node].wildcard = Some(child);
                            child
                        }
                    };
                }
            }
        }

        if self.nodes[node].data.is_some() {
            anyhow::bail!("route '{}' already exists", route);
        }
        self.nodes[node].data = Some(data);

        Ok(())
    }

    pub fn remove(&mut self, route: &str) -> bool {
        if self.static_routes.swap_remove(route).is_some() {
            return true;
        }

        let Some(node) = self.find_dynamic_route_node(route) else {
            return false;
        };
        if self.nodes[node].data.is_none() {
            return false;
        }

        self.nodes[node].data = None;

        if self.nodes[node].has_children() {
            return true;
        }

        let mut current = node;
        while let Some(parent) = self.nodes[current].parent {
            let cur_type = self.nodes[current].node_type;
            match cur_type {
                NodeType::Normal => {
                    if let Some(at) = self.nodes[parent]
                        .normal
                        .iter()
                        .position(|(_, c)| *c == current)
                    {
                        self.nodes[parent].normal.remove(at);
                    }
                }
                NodeType::Placeholder => {
                    if let Some(at) = self.nodes[parent]
                        .placeholder
                        .iter()
                        .position(|(_, c)| *c == current)
                    {
                        self.nodes[parent].placeholder.remove(at);
                    }
                }
                NodeType::Wildcard => {
                    if self.nodes[parent].wildcard == Some(current) {
                        self.nodes[parent].wildcard = None;
                    }
                }
            }

            let parent_stays = self.nodes[parent].data.is_some() || self.nodes[parent].has_children();
            if parent_stays {
                break;
            }

            current = parent;
        }

        true
    }

    pub fn get_route_data(&self, route: &str) -> Option<D> {
        if let Some(data) = self.static_routes.get(route) {
            return Some(data.clone());
        }
        self.find_dynamic_route_node(route)
            .and_then(|node| self.nodes[node].data.clone())
    }

    pub fn contains_route(&self, route: &str) -> bool {
        self.get_route_data(route).is_some()
    }

    fn find_dynamic(
        &self,
        node: usize,
        parts: &[&str],
        index: usize,
        result: &mut Vec<FindResult<D>>,
        params: &mut Vec<(String, String)>,
        only_one: bool,
    ) {
        if only_one && !result.is_empty() {
            return;
        }

        let node_ref = &self.nodes[node];

        // wildcard short-circuit while parts remain
        if !only_one {
            if let Some(wildcard) = node_ref.wildcard {
                if let Some(data) = &self.nodes[wildcard].data {
                    if index < parts.len() {
                        result.push(wildcard_match(data, parts, index, params));
                    }
                }
            }
        }

        if index == parts.len() {
            if let Some(data) = &node_ref.data {
                result.push(FindResult {
                    data: data.clone(),
                    params: Some(params_to_value(params)),
                });
            }

            if only_one && !result.is_empty() {
                return;
            }

            if let Some(wildcard) = node_ref.wildcard {
                if let Some(data) = &self.nodes[wildcard].data {
                    result.push(FindResult {
                        data: data.clone(),
                        params: Some(params_to_value(params)),
                    });
                }
            }
            return;
        }

        let part = parts[index];

        if let Some((_, child)) = node_ref.normal.iter().find(|(k, _)| k == part) {
            let child = *child;
            self.find_dynamic(child, parts, index + 1, result, params, only_one);
        }

        if only_one && !result.is_empty() {
            return;
        }

        if !node_ref.placeholder.is_empty() {
            let placeholders: Vec<(String, usize)> = node_ref.placeholder.clone();
            for (key, child) in placeholders {
                params.push((key, part.to_string()));
                self.find_dynamic(child, parts, index + 1, result, params, only_one);
                params.pop();

                if only_one && !result.is_empty() {
                    return;
                }
            }
        }

        if only_one {
            if let Some(wildcard) = self.nodes[node].wildcard {
                if let Some(data) = &self.nodes[wildcard].data {
                    result.push(wildcard_match(data, parts, index, params));
                }
            }
        }
    }

    pub fn find_one(&self, path: &str) -> Option<FindResult<D>> {
        if let Some(data) = self.static_routes.get(path) {
            return Some(FindResult {
                data: data.clone(),
                params: None,
            });
        }

        let parts = split_parts(path);
        let mut result = Vec::new();
        self.find_dynamic(0, &parts, 0, &mut result, &mut Vec::new(), true);
        result.into_iter().next()
    }

    pub fn find_all(&self, path: &str) -> Vec<FindResult<D>> {
        let parts = split_parts(path);
        let mut result = Vec::new();

        if let Some(data) = self.static_routes.get(path) {
            result.push(FindResult {
                data: data.clone(),
                params: None,
            });
        }

        self.find_dynamic(0, &parts, 0, &mut result, &mut Vec::new(), false);

        result
    }

    pub fn clear(&mut self) {
        self.nodes = vec![Node::new(NodeType::Normal, None)];
        self.static_routes.clear();
    }
}

impl<D: Clone> Default for RadixMatcher<D> {
    fn default() -> Self {
        Self::new()
    }
}

fn params_to_value(params: &[(String, String)]) -> Value {
    let mut map = serde_json::Map::new();
    for (k, v) in params {
        map.insert(k.clone(), Value::String(v.clone()));
    }
    Value::Object(map)
}

/// `{ __: rest-of-path, ...namedParams }`, matching `pushWildcardMatch`.
fn wildcard_match<D: Clone>(data: &D, parts: &[&str], index: usize, params: &[(String, String)]) -> FindResult<D> {
    let mut map = serde_json::Map::new();
    map.insert("__".to_string(), Value::String(parts[index..].join("/")));
    for (k, v) in params {
        map.insert(k.clone(), Value::String(v.clone()));
    }
    FindResult {
        data: data.clone(),
        params: Some(Value::Object(map)),
    }
}
