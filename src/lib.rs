//! kubelens: classify pod and deployment health from `kubectl get -o json`.
use k8s_openapi::api::apps::v1::Deployment;
use k8s_openapi::api::core::v1::{Pod, PodStatus};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PodHealth {
    Ready,
    Pending,
    Done,
    Failing(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rollout {
    Complete,
    InProgress { updated: i32, desired: i32 },
    Stalled,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Summary {
    pub ready: usize,
    pub pending: usize,
    pub done: usize,
    pub failing: usize,
    pub total: usize,
}

fn failing_reason(status: &PodStatus) -> Option<String> {
    status
        .container_statuses
        .iter()
        .flatten()
        .filter_map(|c| {
            let state = c.state.as_ref()?;
            if let Some(t) = state.terminated.as_ref().filter(|t| t.exit_code != 0) {
                return Some(t.reason.clone().unwrap_or_else(|| "Error".to_string()));
            }
            state.waiting.as_ref()?.reason.clone()
        })
        .find(|r| r != "ContainerCreating" && r != "PodInitializing")
}

fn is_ready(status: &PodStatus) -> bool {
    status
        .conditions
        .iter()
        .flatten()
        .any(|c| c.type_ == "Ready" && c.status == "True")
}

pub fn pod_health(pod: &Pod) -> PodHealth {
    let Some(status) = pod.status.as_ref() else {
        return PodHealth::Pending;
    };
    match status.phase.as_deref().unwrap_or("Pending") {
        "Succeeded" => PodHealth::Done,
        "Failed" => PodHealth::Failing("Failed".to_string()),
        phase => match failing_reason(status) {
            Some(reason) => PodHealth::Failing(reason),
            None if phase == "Running" && is_ready(status) => PodHealth::Ready,
            None => PodHealth::Pending,
        },
    }
}
pub fn summarize(pods: &[Pod]) -> Summary {
    let mut s = Summary {
        total: pods.len(),
        ..Summary::default()
    };
    for p in pods {
        match pod_health(p) {
            PodHealth::Ready => s.ready += 1,
            PodHealth::Pending => s.pending += 1,
            PodHealth::Done => s.done += 1,
            PodHealth::Failing(_) => s.failing += 1,
        }
    }
    s
}

pub fn rollout_status(d: &Deployment) -> Rollout {
    let desired = d.spec.as_ref().and_then(|s| s.replicas).unwrap_or(1);
    let st = d.status.clone().unwrap_or_default();
    let stalled = st.conditions.iter().flatten().any(|c| {
        c.type_ == "Progressing" && c.reason.as_deref() == Some("ProgressDeadlineExceeded")
    });
    if stalled {
        return Rollout::Stalled;
    }
    let updated = st.updated_replicas.unwrap_or(0);
    let ready = st.ready_replicas.unwrap_or(0);
    let total = st.replicas.unwrap_or(0);
    if updated == desired && ready == desired && total == desired {
        Rollout::Complete
    } else {
        Rollout::InProgress { updated, desired }
    }
}

/// Contract `pod-summary-partition`: every pod lands in exactly one bucket.
pub fn check_partition(s: &Summary) {
    assert_eq!(s.ready + s.pending + s.done + s.failing, s.total);
}

fn parse_list<T: serde::de::DeserializeOwned>(input: &str) -> Result<Vec<T>, String> {
    let v: serde_json::Value =
        serde_json::from_str(input).map_err(|e| format!("invalid kubectl json: {e}"))?;
    let items = v
        .get("items")
        .cloned()
        .unwrap_or_else(|| serde_json::json!([]));
    serde_json::from_value(items).map_err(|e| format!("invalid kubectl json: {e}"))
}

fn name_of(m: &k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta) -> String {
    m.name.clone().unwrap_or_default()
}

fn run_pods(input: &str) -> (String, i32) {
    let pods = match parse_list::<Pod>(input) {
        Ok(p) => p,
        Err(e) => return (format!("{e}\n"), 2),
    };
    let s = summarize(&pods);
    check_partition(&s);
    let mut out = String::new();
    for p in &pods {
        out.push_str(&format!("{}\t{:?}\n", name_of(&p.metadata), pod_health(p)));
    }
    out.push_str(&format!(
        "ready={} pending={} done={} failing={} total={}\n",
        s.ready, s.pending, s.done, s.failing, s.total
    ));
    out.push_str("contract: pod-summary-partition OK\n");
    (out, i32::from(s.failing > 0))
}

fn run_deploys(input: &str) -> (String, i32) {
    let ds = match parse_list::<Deployment>(input) {
        Ok(d) => d,
        Err(e) => return (format!("{e}\n"), 2),
    };
    let mut out = String::new();
    let mut bad = false;
    for d in &ds {
        let r = rollout_status(d);
        bad |= r == Rollout::Stalled;
        out.push_str(&format!("{}\t{r:?}\n", name_of(&d.metadata)));
    }
    (out, i32::from(bad))
}

/// Run a subcommand over JSON input. Returns (stdout, exit code).
pub fn run(args: &[String], input: &str) -> (String, i32) {
    match args.first().map(String::as_str) {
        Some("pods") => run_pods(input),
        Some("deploys") => run_deploys(input),
        _ => (
            "usage: kubelens <pods|deploys> < kubectl-json\n".to_string(),
            2,
        ),
    }
}
