use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceDiffFileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Conflicted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkspaceDiffContent {
    Text { patch: String },
    Binary,
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: WorkspaceDiffFileStatus,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub additions: usize,
    pub deletions: usize,
    pub content: WorkspaceDiffContent,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceDiffSnapshot {
    pub files: Vec<WorkspaceDiffFile>,
    pub truncated: bool,
}

/// Ownership-trust state of the repository backing the bound workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GitTrustState {
    /// Git accepts the repository as-is.
    Trusted,
    /// The repository exists but Git rejects it on ownership grounds until the
    /// user grants trust.
    TrustRequired,
    /// No repository was found at the bound workspace.
    NotARepository,
}

/// Read-only probe of repository ownership trust. Mirrors the git service
/// trust report; nothing is configured by probing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitTrustSnapshot {
    pub state: GitTrustState,
    /// Path Git validates ownership against, when it is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_path: Option<String>,
    /// Provider diagnostic, preserved for surfaces that show manual steps.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// Command the user can run to resolve this outside the product.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual_command: Option<String>,
}

/// One entry of the recent-commit history of the current branch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitCommitSummary {
    pub hash: String,
    pub short_hash: String,
    /// First line of the commit message.
    pub subject: String,
    pub author: String,
    /// Provider-formatted commit timestamp.
    pub date: String,
}

/// Branch and remote facts of the bound workspace.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitRepositoryOverview {
    pub current_branch: String,
    pub has_changes: bool,
    pub remotes: Vec<String>,
}

#[async_trait::async_trait]
pub trait GitPort: RuntimeServicePort {
    async fn workspace_diff(&self) -> PortResult<WorkspaceDiffSnapshot> {
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "workspace diff is not supported by this provider",
        ))
    }

    /// Stages files (`git add`). `all` stages every change including
    /// untracked files, `update` stages tracked modifications only, and
    /// otherwise `files` names the workspace-relative paths to stage.
    async fn stage_files(&self, files: Vec<String>, all: bool, update: bool) -> PortResult<()> {
        let _ = (files, all, update);
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "staging is not supported by this provider",
        ))
    }

    /// Unstages files (`git restore --staged`); the working tree is untouched.
    async fn unstage_files(&self, files: Vec<String>) -> PortResult<()> {
        let _ = files;
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "unstaging is not supported by this provider",
        ))
    }

    /// Commits the staged snapshot; `stage_all` mirrors `git commit -a`.
    async fn commit_staged(&self, message: String, stage_all: bool) -> PortResult<()> {
        let _ = (message, stage_all);
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "commit is not supported by this provider",
        ))
    }

    /// Pushes the current branch. `remote`/`branch` fall back to Git's own
    /// defaults when omitted; `set_upstream` mirrors `git push -u`.
    async fn push(
        &self,
        remote: Option<String>,
        branch: Option<String>,
        set_upstream: bool,
    ) -> PortResult<()> {
        let _ = (remote, branch, set_upstream);
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "push is not supported by this provider",
        ))
    }

    /// Recent commits of the current branch, newest first.
    async fn commit_history(&self, limit: usize) -> PortResult<Vec<GitCommitSummary>> {
        let _ = limit;
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "commit history is not supported by this provider",
        ))
    }

    /// Branch and remote facts of the bound workspace.
    async fn repository_overview(&self) -> PortResult<GitRepositoryOverview> {
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "repository overview is not supported by this provider",
        ))
    }

    /// Probes repository ownership trust without changing anything. Write
    /// attempts on an untrusted repository fail with
    /// [`PortErrorKind::PermissionDenied`]; surfaces should probe before
    /// offering the trust decision.
    async fn trust_report(&self) -> PortResult<GitTrustSnapshot> {
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "trust report is not supported by this provider",
        ))
    }

    /// Applies an explicitly confirmed trust decision (writes
    /// `safe.directory`) and reports the state observed afterwards.
    async fn trust_repository(&self) -> PortResult<GitTrustSnapshot> {
        Err(PortError::new(
            PortErrorKind::NotAvailable,
            "trusting a repository is not supported by this provider",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_trust_snapshot_serializes_camel_case_with_sparse_fields() {
        let snapshot = GitTrustSnapshot {
            state: GitTrustState::TrustRequired,
            repository_path: Some("/srv/repo".into()),
            detail: None,
            manual_command: Some("git config --global --add safe.directory /srv/repo".into()),
        };

        let json = serde_json::to_value(&snapshot).expect("serialize trust snapshot");

        assert_eq!(json["state"], "trust_required");
        assert_eq!(json["repositoryPath"], "/srv/repo");
        assert_eq!(
            json["manualCommand"],
            "git config --global --add safe.directory /srv/repo"
        );
        assert!(json.get("detail").is_none());

        let round_tripped: GitTrustSnapshot =
            serde_json::from_value(json).expect("deserialize trust snapshot");
        assert_eq!(round_tripped, snapshot);
    }

    #[test]
    fn git_commit_summary_and_overview_round_trip() {
        let commit = GitCommitSummary {
            hash: "4911b0a40".into(),
            short_hash: "4911b0a".into(),
            subject: "feat: terminal tabs".into(),
            author: "re2zero".into(),
            date: "2026-10-10 12:00:00".into(),
        };
        let json = serde_json::to_value(&commit).expect("serialize commit summary");
        assert_eq!(json["shortHash"], "4911b0a");
        let round_tripped: GitCommitSummary =
            serde_json::from_value(json).expect("deserialize commit summary");
        assert_eq!(round_tripped, commit);

        let overview = GitRepositoryOverview {
            current_branch: "gpui-frontend".into(),
            has_changes: true,
            remotes: vec!["origin".into()],
        };
        let json = serde_json::to_value(&overview).expect("serialize overview");
        assert_eq!(json["currentBranch"], "gpui-frontend");
        assert_eq!(json["hasChanges"], true);
        let round_tripped: GitRepositoryOverview =
            serde_json::from_value(json).expect("deserialize overview");
        assert_eq!(round_tripped, overview);
    }

    /// The default port is a capability marker: every new operation reports
    /// `NotAvailable` instead of forcing providers to stub them.
    #[tokio::test]
    async fn git_port_defaults_report_not_available() {
        struct MarkerPort;

        impl RuntimeServicePort for MarkerPort {
            fn capability(&self) -> RuntimeServiceCapability {
                RuntimeServiceCapability::Git
            }
        }

        #[async_trait::async_trait]
        impl GitPort for MarkerPort {}

        let port = MarkerPort;
        assert!(matches!(
            port.workspace_diff().await,
            Err(error) if error.kind == PortErrorKind::NotAvailable
        ));
        assert!(port
            .stage_files(vec!["a.txt".into()], false, false)
            .await
            .is_err());
        assert!(port.unstage_files(vec!["a.txt".into()]).await.is_err());
        assert!(port.commit_staged("msg".into(), false).await.is_err());
        assert!(port.push(None, None, false).await.is_err());
        assert!(port.commit_history(10).await.is_err());
        assert!(port.repository_overview().await.is_err());
        assert!(port.trust_report().await.is_err());
        assert!(port.trust_repository().await.is_err());
    }
}
