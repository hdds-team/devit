// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2025-2026 naskel.com

//! Workspace management

use std::path::PathBuf;

/// Represents an open workspace (folder)
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Root path of the workspace
    pub root: PathBuf,
    /// Detected project type
    pub project_type: Option<ProjectType>,
}

#[derive(Debug, Clone)]
pub enum ProjectType {
    Rust,   // Cargo.toml
    Node,   // package.json
    Python, // pyproject.toml, setup.py, requirements.txt
    Go,     // go.mod
    Unknown,
}

impl Workspace {
    pub fn new(root: PathBuf) -> Self {
        let project_type = Self::detect_project_type(&root);
        Self { root, project_type }
    }

    fn detect_project_type(root: &PathBuf) -> Option<ProjectType> {
        if root.join("Cargo.toml").exists() {
            Some(ProjectType::Rust)
        } else if root.join("package.json").exists() {
            Some(ProjectType::Node)
        } else if root.join("pyproject.toml").exists()
            || root.join("setup.py").exists()
            || root.join("requirements.txt").exists()
        {
            Some(ProjectType::Python)
        } else if root.join("go.mod").exists() {
            Some(ProjectType::Go)
        } else {
            Some(ProjectType::Unknown)
        }
    }

    /// Get the primary language for this workspace
    pub fn primary_language(&self) -> Option<&'static str> {
        match self.project_type {
            Some(ProjectType::Rust) => Some("rust"),
            Some(ProjectType::Node) => Some("typescript"),
            Some(ProjectType::Python) => Some("python"),
            Some(ProjectType::Go) => Some("go"),
            _ => None,
        }
    }
}
