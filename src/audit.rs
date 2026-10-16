//! Least-privilege audits over kubectl JSON: RBAC roles and rendered manifests.
use serde_json::Value;

#[derive(Debug, PartialEq)]
pub struct Finding {
    pub subject: String,
    pub problem: String,
}

fn name_of(v: &Value) -> String {
    v.pointer("/metadata/name")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_string()
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

const DANGEROUS: [&str; 3] = ["escalate", "bind", "impersonate"];
const READS: [&str; 3] = ["get", "list", "watch"];

/// Audit the rules of one (Cluster)Role.
pub fn role_findings(role: &Value) -> Vec<Finding> {
    let subject = name_of(role);
    let mut out = Vec::new();
    let rules = role.get("rules").and_then(Value::as_array);
    for rule in rules.into_iter().flatten() {
        let verbs = strings(rule.get("verbs"));
        let resources = strings(rule.get("resources"));
        let mut add = |p: &str| {
            out.push(Finding {
                subject: subject.clone(),
                problem: p.to_string(),
            })
        };
        if verbs.iter().any(|v| v == "*") {
            add("wildcard verb");
        }
        if resources.iter().any(|r| r == "*") {
            add("wildcard resource");
        }
        if verbs.iter().any(|v| DANGEROUS.contains(&v.as_str())) {
            add("privilege escalation verb");
        }
        let reads = verbs.iter().any(|v| READS.contains(&v.as_str()));
        if reads && resources.iter().any(|r| r == "secrets") {
            add("reads secrets");
        }
    }
    out
}

/// Audit a `kubectl get clusterroles -o json` list.
pub fn rbac_findings(list: &Value) -> Vec<Finding> {
    let items = list.get("items").and_then(Value::as_array);
    items
        .into_iter()
        .flatten()
        .flat_map(role_findings)
        .collect()
}

/// Audit the containers of every Deployment in a list of rendered manifests.
pub fn manifest_findings(list: &Value) -> Vec<Finding> {
    let mut out = Vec::new();
    let items = list.get("items").and_then(Value::as_array);
    for item in items.into_iter().flatten() {
        if item.get("kind").and_then(Value::as_str) != Some("Deployment") {
            continue;
        }
        let subject = name_of(item);
        let cs = item.pointer("/spec/template/spec/containers");
        for c in cs.and_then(Value::as_array).into_iter().flatten() {
            let image = c.get("image").and_then(Value::as_str).unwrap_or("");
            let tag = image.rsplit_once(':').map(|(_, t)| t);
            if tag.is_none() || tag == Some("latest") {
                out.push(Finding {
                    subject: subject.clone(),
                    problem: "image not pinned".into(),
                });
            }
            if c.pointer("/resources/limits").is_none() {
                out.push(Finding {
                    subject: subject.clone(),
                    problem: "no resource limits".into(),
                });
            }
        }
    }
    out
}

/// Contract `least-privilege`: a clean audit means no wildcard anywhere in the input.
pub fn check_least_privilege(list: &Value, findings: &[Finding]) {
    if findings.is_empty() {
        assert!(!list.to_string().contains("\"*\""));
    }
}

/// Render findings as `subject<TAB>problem` lines.
pub fn render(findings: &[Finding]) -> String {
    findings
        .iter()
        .map(|f| format!("{}\t{}\n", f.subject, f.problem))
        .collect()
}
