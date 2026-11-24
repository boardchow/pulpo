//! Linear's open issues, read with a personal API key, to start a workspace on one.

use std::io::Write;
use std::process::{Command, Stdio};

use serde_json::Value;

const QUERY: &str = "query($after: String) { viewer { name } issues(first: 250, after: $after, orderBy: updatedAt, filter: { state: { type: { nin: [\"completed\", \"canceled\"] } } }) { pageInfo { hasNextPage endCursor } nodes { identifier title url branchName project { name } assignee { name } state { name } } } }";

/// shortcut: the newest 1000 open issues, no more pages; filter on the server when that isn't enough.
const MAX_PAGES: usize = 4;

const SEARCH_QUERY: &str = "query($term: String!, $filter: IssueFilter) { searchIssues(term: $term, first: 50, includeComments: true, filter: $filter) { nodes { identifier title url branchName project { name } assignee { name } state { name } } } }";

const FILTER_QUERY: &str = "query($filter: IssueFilter) { issues(first: 50, orderBy: updatedAt, filter: $filter) { nodes { identifier title url branchName project { name } assignee { name } state { name } } } }";

const STATE_TYPES: [&str; 6] = ["triage", "backlog", "unstarted", "started", "completed", "canceled"];

const DETAIL_QUERY: &str = "query($id: String!) { issue(id: $id) { description priorityLabel estimate cycle { number name } labels { nodes { name } } parent { identifier title } children { nodes { identifier title state { name } } } attachments { nodes { title url } } comments(first: 50) { nodes { body createdAt user { name } } } } }";

const DEFAULT_PROMPT: &str = "Work on Linear issue {id}";

#[derive(Clone, Debug, PartialEq)]
pub struct Issue {
    pub id: String,
    pub title: String,
    pub url: String,
    pub branch: String,
    pub project: String,
    pub assignee: String,
    pub state: String,
}

pub struct Loaded {
    pub issues: Vec<Issue>,
    pub me: String,
}

impl Issue {
    /// The workspace's name, when the issue's number is one.
    pub fn workspace_name(&self) -> Option<String> {
        self.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-').then(|| self.id.to_lowercase()).filter(|name| !name.is_empty())
    }
}

/// What the agent in the issue's workspace is started with: `$PULPO_LINEAR_PROMPT`, where `{id}` is the issue's number.
pub fn start_command(shell: &str, id: &str) -> String {
    start_with(shell, id, &std::env::var("PULPO_LINEAR_PROMPT").unwrap_or_else(|_| DEFAULT_PROMPT.into()))
}

fn start_with(shell: &str, id: &str, prompt: &str) -> String {
    format!("claude {}; exec {} -l", quote(&prompt.replace("{id}", id)), quote(shell))
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

impl Loaded {
    pub fn new(me: String, issues: Vec<Issue>) -> Loaded {
        Loaded { issues, me }
    }

    /// With nothing typed, your issues (or everyone's); with something, any issue that fits it.
    pub fn visible(&self, query: &str, everyone: bool) -> Vec<&Issue> {
        let query = Query::parse(query, &self.me);
        self.issues.iter().filter(|i| !query.is_empty() || everyone || i.assignee == self.me).filter(|i| query.matches(i)).collect()
    }
}

/// What is typed: words to find, and `@assignee`, `#project` and `is:state` to narrow by.
#[derive(Default)]
pub struct Query {
    pub words: Vec<String>,
    assignee: Option<String>,
    project: Option<String>,
    state: Option<String>,
}

impl Query {
    pub fn parse(text: &str, me: &str) -> Query {
        let mut query = Query::default();
        for word in text.split_whitespace() {
            let lower = word.to_lowercase();
            if matches!(lower.as_str(), "@" | "#" | "is:") {
                continue;
            }
            match (lower.strip_prefix('@'), lower.strip_prefix('#'), lower.strip_prefix("is:")) {
                (Some("me"), ..) if !me.is_empty() => query.assignee = Some(me.to_lowercase()),
                (Some(name), ..) if !name.is_empty() => query.assignee = Some(name.to_string()),
                (_, Some(project), _) if !project.is_empty() => query.project = Some(project.to_string()),
                (.., Some(state)) if !state.is_empty() => query.state = Some(state.to_string()),
                _ => query.words.push(lower),
            }
        }
        query
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.assignee.is_none() && self.project.is_none() && self.state.is_none()
    }

    pub fn matches(&self, issue: &Issue) -> bool {
        let has = |field: &str, wanted: &Option<String>| wanted.as_ref().is_none_or(|w| field.to_lowercase().contains(w));
        let text = format!("{} {} {} {} {}", issue.id, issue.title, issue.project, issue.assignee, issue.state).to_lowercase();
        has(&issue.assignee, &self.assignee) && has(&issue.project, &self.project) && has(&issue.state, &self.state) && self.words.iter().all(|w| text.contains(w))
    }

    /// What Linear is asked for, so an answer can be told from one for something typed since.
    pub fn key(&self) -> String {
        let tokens = [self.assignee.as_ref().map(|a| format!("@{a}")), self.project.as_ref().map(|p| format!("#{p}")), self.state.as_ref().map(|s| format!("is:{s}"))];
        self.words.iter().cloned().chain(tokens.into_iter().flatten()).collect::<Vec<_>>().join(" ")
    }

    /// The tokens as Linear's issue filter; open issues only, unless a state is asked for.
    fn filter(&self) -> Value {
        let named = |name: &str| serde_json::json!({ "name": { "containsIgnoreCase": name } });
        let mut filter = serde_json::Map::new();
        if let Some(assignee) = &self.assignee {
            filter.insert("assignee".into(), named(assignee));
        }
        if let Some(project) = &self.project {
            filter.insert("project".into(), named(project));
        }
        let state = match &self.state {
            None => serde_json::json!({ "type": { "nin": ["completed", "canceled"] } }),
            Some(state) => {
                let kind = if state == "done" { "completed" } else { state.as_str() };
                match STATE_TYPES.contains(&kind) {
                    true => serde_json::json!({ "or": [{ "name": { "containsIgnoreCase": state } }, { "type": { "eq": kind } }] }),
                    false => named(state),
                }
            }
        };
        filter.insert("state".into(), state);
        Value::Object(filter)
    }
}

pub fn api_key() -> Option<String> {
    let key = std::env::var("LINEAR_API_KEY").ok().or_else(|| std::fs::read_to_string(crate::project::config_dir().join("linear")).ok())?;
    Some(key.trim().to_string()).filter(|key| !key.is_empty())
}

pub fn fetch(key: &str) -> Result<Loaded, String> {
    let (mut me, mut issues, mut after) = (String::new(), Vec::new(), None);
    for _ in 0..MAX_PAGES {
        let page = page(key, after.as_deref())?;
        (me, after) = (page.me, page.next);
        issues.extend(page.issues);
        if after.is_none() {
            break;
        }
    }
    Ok(Loaded::new(me, issues))
}

fn page(key: &str, after: Option<&str>) -> Result<Page, String> {
    parse(&post(key, &serde_json::json!({ "query": QUERY, "variables": { "after": after } }))?)
}

pub fn fetch_detail(key: &str, id: &str) -> Result<Detail, String> {
    parse_detail(&post(key, &serde_json::json!({ "query": DETAIL_QUERY, "variables": { "id": id } }))?)
}

fn post(key: &str, body: &Value) -> Result<Vec<u8>, String> {
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        return Err("the Linear API key has characters a key doesn't".into());
    }
    let body = body.to_string();
    let mut curl = Command::new("curl")
        .args(["-sS", "--max-time", "20", "-K", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run curl: {e}"))?;
    curl.stdin.take().ok_or("no stdin")?.write_all(curl_config(key, &body).as_bytes()).map_err(|e| e.to_string())?;
    let out = curl.wait_with_output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("could not reach Linear: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// Read by curl from stdin, so the key isn't in `ps`.
fn curl_config(key: &str, body: &str) -> String {
    format!(
        "url = \"https://api.linear.app/graphql\"\nheader = \"Authorization: {key}\"\nheader = \"Content-Type: application/json\"\ndata-binary = \"{}\"\n",
        body.replace('\\', "\\\\").replace('"', "\\\"")
    )
}

/// What reading an issue shows, past what the list has.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Detail {
    pub description: String,
    pub priority: String,
    pub estimate: Option<f64>,
    pub cycle: String,
    pub labels: Vec<String>,
    pub parent: String,
    pub children: Vec<String>,
    pub attachments: Vec<String>,
    pub comments: Vec<Comment>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub author: String,
    pub day: String,
    pub body: String,
}

fn parse_detail(body: &[u8]) -> Result<Detail, String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("Linear answered with something unreadable: {e}"))?;
    if let Some(error) = v["errors"][0]["message"].as_str() {
        return Err(format!("Linear says: {error}"));
    }
    let issue = &v["data"]["issue"];
    if issue.is_null() {
        return Err("Linear has no such issue".into());
    }
    let text = |v: &Value| v.as_str().unwrap_or_default().replace('\r', "").chars().filter(|c| !c.is_control() || *c == '\n').collect::<String>();
    let one = |v: &Value| text(v).replace('\n', " ");
    let nodes = |v: &Value| v["nodes"].as_array().cloned().unwrap_or_default();
    let cycle = &issue["cycle"];
    let cycle = format!("{} {}", cycle["number"].as_i64().map(|n| n.to_string()).unwrap_or_default(), one(&cycle["name"])).trim().to_string();
    Ok(Detail {
        description: text(&issue["description"]),
        priority: one(&issue["priorityLabel"]),
        estimate: issue["estimate"].as_f64(),
        cycle,
        labels: nodes(&issue["labels"]).iter().map(|n| one(&n["name"])).collect(),
        parent: if issue["parent"].is_null() { String::new() } else { format!("{} {}", one(&issue["parent"]["identifier"]), one(&issue["parent"]["title"])) },
        children: nodes(&issue["children"]).iter().map(|n| format!("{} {} ({})", one(&n["identifier"]), one(&n["title"]), one(&n["state"]["name"]))).collect(),
        attachments: nodes(&issue["attachments"]).iter().map(|n| format!("{} {}", one(&n["title"]), one(&n["url"]))).collect(),
        comments: nodes(&issue["comments"])
            .iter()
            .map(|n| Comment { author: one(&n["user"]["name"]), day: text(&n["createdAt"]).chars().take(10).collect(), body: text(&n["body"]) })
            .collect(),
    })
}

struct Page {
    me: String,
    issues: Vec<Issue>,
    next: Option<String>,
}

fn issues_in(nodes: &Value) -> Result<Vec<Issue>, String> {
    let text = |v: &Value| v.as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).collect::<String>();
    let nodes = nodes.as_array().ok_or("Linear's answer has no issues")?;
    Ok(nodes
        .iter()
        .map(|n| Issue {
            id: text(&n["identifier"]),
            title: text(&n["title"]),
            url: text(&n["url"]),
            branch: text(&n["branchName"]),
            project: text(&n["project"]["name"]),
            assignee: text(&n["assignee"]["name"]),
            state: text(&n["state"]["name"]),
        })
        .collect())
}

/// Linear's own full-text search: titles, descriptions and comments, across the workspace.
pub fn search(key: &str, query: &Query) -> Result<Vec<Issue>, String> {
    let words = query.words.join(" ");
    let (graphql, variables) = match words.is_empty() {
        true => (FILTER_QUERY, serde_json::json!({ "filter": query.filter() })),
        false => (SEARCH_QUERY, serde_json::json!({ "term": words, "filter": query.filter() })),
    };
    let v: Value = serde_json::from_slice(&post(key, &serde_json::json!({ "query": graphql, "variables": variables }))?)
        .map_err(|e| format!("Linear answered with something unreadable: {e}"))?;
    if let Some(error) = v["errors"][0]["message"].as_str() {
        return Err(format!("Linear says: {error}"));
    }
    issues_in(if words.is_empty() { &v["data"]["issues"]["nodes"] } else { &v["data"]["searchIssues"]["nodes"] })
}

fn parse(body: &[u8]) -> Result<Page, String> {
    let v: Value = serde_json::from_slice(body).map_err(|e| format!("Linear answered with something unreadable: {e}"))?;
    if let Some(error) = v["errors"][0]["message"].as_str() {
        return Err(format!("Linear says: {error}"));
    }
    let text = |v: &Value| v.as_str().unwrap_or_default().chars().filter(|c| !c.is_control()).collect::<String>();
    let issues = &v["data"]["issues"];
    let issues_read = issues_in(&issues["nodes"])?;
    let next = issues["pageInfo"]["hasNextPage"].as_bool().unwrap_or(false).then(|| issues["pageInfo"]["endCursor"].as_str().map(String::from)).flatten();
    Ok(Page { me: text(&v["data"]["viewer"]["name"]), issues: issues_read, next })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANSWER: &str = r#"{"data":{"viewer":{"name":"Sam"},"issues":{"nodes":[
        {"identifier":"APP-52","title":"Add debug build","branchName":"sam/app-52-add-debug-build","project":{"name":"Mobile"},"assignee":{"name":"Sam"},"state":{"name":"Todo"}},
        {"identifier":"WEB-68","title":"Add lifting tabs","branchName":"sam/web-68","project":{"name":"Web App"},"assignee":{"name":"Maria"},"state":{"name":"In Progress"}},
        {"identifier":"WEB-70","title":"Gym audit","branchName":"sam/web-70","project":null,"assignee":null,"state":{"name":"Todo"}}]}}}"#;

    #[test]
    fn reads_issues_and_narrows_them_by_whose_and_words() {
        let page = parse(ANSWER.as_bytes()).unwrap();
        assert_eq!(page.next, None);
        let loaded = Loaded::new(page.me, page.issues);
        let ids = |query, everyone| loaded.visible(query, everyone).iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids("", false), vec!["APP-52"]);
        assert_eq!(ids("", true), vec!["APP-52", "WEB-68", "WEB-70"]);
        assert_eq!(ids("lifting", false), vec!["WEB-68"]);
        assert_eq!(ids("todo", false), vec!["APP-52", "WEB-70"]);
        assert_eq!(loaded.issues[0].branch, "sam/app-52-add-debug-build");
    }

    #[test]
    fn follows_the_cursor_and_keeps_control_characters_out() {
        let more = r#"{"data":{"viewer":{"name":"Sam"},"issues":{"pageInfo":{"hasNextPage":true,"endCursor":"abc="},"nodes":[
            {"identifier":"APP-1","title":"a\u001b[31mred","branchName":"b","project":null,"assignee":null,"state":null}]}}}"#;
        let page = parse(more.as_bytes()).unwrap();
        assert_eq!(page.next.as_deref(), Some("abc="));
        assert_eq!(page.issues[0].title, "a[31mred");
    }

    #[test]
    fn escapes_the_body_for_curl_and_quotes_what_the_shell_runs() {
        let config = curl_config("lin_api_x", r#"{"q":"a\\b \"c\""}"#);
        assert!(config.contains(r#"data-binary = "{\"q\":\"a\\\\b \\\"c\\\"\"}""#), "{config}");
        assert!(config.contains("header = \"Authorization: lin_api_x\"\n"));
        assert_eq!(start_with("/bin/zsh", "APP-52", DEFAULT_PROMPT), "claude 'Work on Linear issue APP-52'; exec '/bin/zsh' -l");
        assert_eq!(start_with("/opt/my sh/zsh", "X-1", "it's {id}"), "claude 'it'\\''s X-1'; exec '/opt/my sh/zsh' -l");
        assert_eq!(quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn names_a_workspace_after_an_issue_number_only() {
        let issue = |id: &str| Issue { id: id.into(), title: String::new(), url: String::new(), branch: String::new(), project: String::new(), assignee: String::new(), state: String::new() };
        assert_eq!(issue("APP-52").workspace_name().as_deref(), Some("app-52"));
        assert_eq!(issue("APP-52; rm").workspace_name(), None);
        assert_eq!(issue("").workspace_name(), None);
    }

    #[test]
    fn narrows_by_typed_tokens_and_ignores_the_chosen_filters_once_typing() {
        let loaded = parse(ANSWER.as_bytes()).map(|p| Loaded::new(p.me, p.issues)).unwrap();
        let ids = |query: &str| loaded.visible(query, false).iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(""), vec!["APP-52"]);
        assert_eq!(ids("@maria"), vec!["WEB-68"]);
        assert_eq!(ids("#web"), vec!["WEB-68"]);
        assert_eq!(ids("is:todo"), vec!["APP-52", "WEB-70"]);
        assert_eq!(ids("is:todo @me"), vec!["APP-52"]);
        assert_eq!(ids("lifting #web is:progress"), vec!["WEB-68"]);
        assert_eq!(ids("gym"), vec!["WEB-70"]);
        assert_eq!(Query::parse("gym @maria #x lift is:todo", "Sam").words, vec!["gym", "lift"]);
    }

    #[test]
    fn asks_linear_for_the_typed_tokens_and_for_done_issues_when_told() {
        let query = Query::parse("login @Riley is:done", "Sam");
        assert_eq!(query.key(), "login @riley is:done");
        assert_eq!(
            query.filter(),
            serde_json::json!({
                "assignee": { "name": { "containsIgnoreCase": "riley" } },
                "state": { "or": [{ "name": { "containsIgnoreCase": "done" } }, { "type": { "eq": "completed" } }] },
            })
        );
        assert_eq!(Query::parse("#web", "Sam").filter()["state"], serde_json::json!({ "type": { "nin": ["completed", "canceled"] } }));
        assert_eq!(Query::parse("is:review", "Sam").filter()["state"], serde_json::json!({ "name": { "containsIgnoreCase": "review" } }));
    }

    #[test]
    fn reads_an_issue_to_read() {
        let answer = r##"{"data":{"issue":{"description":"# Why\r\nBecause.","priorityLabel":"High","estimate":3,"cycle":{"number":12,"name":null},
            "labels":{"nodes":[{"name":"ios"}]},"parent":{"identifier":"APP-1","title":"Epic"},
            "children":{"nodes":[{"identifier":"APP-53","title":"Sub","state":{"name":"Todo"}}]},
            "attachments":{"nodes":[{"title":"PR 7","url":"https://github.com/x/y/pull/7"}]},
            "comments":{"nodes":[{"body":"Looks good","createdAt":"2026-11-20T10:00:00.000Z","user":{"name":"Maria"}}]}}}}"##;
        let detail = parse_detail(answer.as_bytes()).unwrap();
        assert_eq!(detail.description, "# Why\nBecause.");
        assert_eq!((detail.priority.as_str(), detail.estimate, detail.cycle.as_str()), ("High", Some(3.0), "12"));
        assert_eq!(detail.parent, "APP-1 Epic");
        assert_eq!(detail.children, vec!["APP-53 Sub (Todo)"]);
        assert_eq!(detail.comments, vec![Comment { author: "Maria".into(), day: "2026-11-20".into(), body: "Looks good".into() }]);
        assert!(parse_detail(br#"{"data":{"issue":null}}"#).is_err());
    }

    #[test]
    fn reports_what_linear_refuses() {
        let refused = br#"{"errors":[{"message":"Authentication required"}]}"#;
        assert_eq!(parse(refused).err().unwrap(), "Linear says: Authentication required");
        assert!(parse(b"<html>").is_err());
    }
}
