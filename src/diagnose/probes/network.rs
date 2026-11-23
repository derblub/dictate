// Copyright (C) 2026 Daniel Kurdoghlian
// Pushing Pixels, Vienna <https://pushingpixels.at>
//
// This program is free software; you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation; either version 3 of the License, or (at your option)
// any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU General Public License
// for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program; if not, see <https://www.gnu.org/licenses>.
//
// Additional permission under GNU GPL version 3 section 7
//
// If you modify this Program, or any covered work, by linking or combining
// it with NVIDIA CUDA (the libraries of the NVIDIA CUDA Toolkit, such as
// libcudart, libcublas and libcublasLt, and the CUDA driver library libcuda
// of the NVIDIA driver) (or a modified version of that library), containing
// parts covered by the terms of the NVIDIA CUDA Toolkit End User License
// Agreement or the NVIDIA Driver License Agreement, the licensors of this
// Program grant you additional permission to convey the resulting work.

//! Proben der Gruppe Network: `net_links`, `net_dns`, `net_listen`, `net_tailscale`, `net_mounts`, `net_reach`.
//! `net_reach` ist die einzige, die Netzverkehr erzeugt (nur auf Bestätigung, Ziele nur aus der Config).

use super::collect::{ProbeResult, Run, RAW_LIMIT};
use super::common::{commands, is_silent_no_match, plain_text_probe, single, table};
use super::json;
use super::output::{Part, Status};
use super::text::{cut_to_bytes, human_bytes};
use crate::diagnose::pseudo::Category;
use crate::diagnose::run::Keep;
use serde_json::Value;

fn str_field(v: &Value, key: &str) -> String {
    json::str_of(v, key).unwrap_or("").to_string()
}

// ---- net_links -------------------------------------------------------------------------------------------------------

/// Eine Zeile je Schnittstelle; `veth*` ohne Adresse (ein Paar je Container) werden gezählt statt aufgelistet.
fn links_text(doc: &Value) -> Option<String> {
    let ifaces = doc.as_array()?;
    let mut out = String::new();
    let mut veth: std::collections::BTreeMap<String, usize> = Default::default();
    for i in ifaces {
        let name = str_field(i, "ifname");
        let addrs = i.get("addr_info").and_then(Value::as_array).cloned().unwrap_or_default();
        let state = str_field(i, "operstate");
        if name.starts_with("veth") && addrs.is_empty() {
            *veth.entry(state).or_default() += 1;
            continue;
        }
        let flags: Vec<&str> = i.get("flags").and_then(Value::as_array).map(|f| f.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let important: Vec<&str> = flags.iter().copied().filter(|f| ["UP", "LOWER_UP", "NO-CARRIER", "PROMISC"].contains(f)).collect();
        out.push_str(&format!(
            "{}: {} {} mtu {}",
            i.get("ifindex").and_then(Value::as_i64).unwrap_or(0),
            name,
            state,
            i.get("mtu").and_then(Value::as_i64).unwrap_or(0)
        ));
        if !important.is_empty() {
            out.push_str(&format!(" <{}>", important.join(",")));
        }
        let mac = str_field(i, "address");
        if !mac.is_empty() && mac != "00:00:00:00:00:00" {
            out.push_str(&format!(" mac {mac}"));
        }
        for a in &addrs {
            out.push_str(&format!(" {} {}/{}", str_field(a, "family"), str_field(a, "local"), a.get("prefixlen").and_then(Value::as_i64).unwrap_or(0)));
            let scope = str_field(a, "scope");
            if scope == "host" || scope == "link" {
                out.push_str(&format!(" ({scope})"));
            }
        }
        out.push('\n');
    }
    if !veth.is_empty() {
        let total: usize = veth.values().sum();
        let states = veth.iter().map(|(s, n)| format!("{n} {s}")).collect::<Vec<_>>().join(", ");
        out.push_str(&format!("veth*: {total} interfaces without address ({states})\n"));
    }
    Some(out)
}

fn routes_text(doc: &Value) -> Option<String> {
    let routes = doc.as_array()?;
    let mut out = String::new();
    for r in routes {
        out.push_str(&str_field(r, "dst"));
        for (key, label) in [("gateway", "via"), ("dev", "dev"), ("protocol", "proto"), ("scope", "scope"), ("prefsrc", "src")] {
            let v = str_field(r, key);
            if !v.is_empty() {
                out.push_str(&format!(" {label} {v}"));
            }
        }
        if let Some(m) = r.get("metric").and_then(Value::as_i64) {
            out.push_str(&format!(" metric {m}"));
        }
        out.push('\n');
    }
    Some(out)
}

pub fn net_links(run: &mut Run) -> ProbeResult {
    let argvs = commands(run)?;
    let labels = ["addresses", "routes"];
    let mut failures = Vec::new();
    let mut used = 0usize;
    for (i, argv) in argvs.iter().enumerate() {
        let cap = run.share(used, i, argvs.len());
        let cmd = run.exec(argv, RAW_LIMIT, Keep::Head);
        match cmd.problem() {
            Some(Status::TimedOut) => return Err(Status::TimedOut),
            Some(Status::Cancelled) => return Err(Status::Cancelled),
            Some(other) => {
                failures.push(format!("{}: {}", labels[i], other.message().unwrap_or(other.label())));
                continue;
            }
            None => {}
        }
        let raw = cmd.raw_text();
        let rendered = json::document(&raw).and_then(|d| if i == 0 { links_text(&d) } else { routes_text(&d) });
        let text = match rendered {
            Some(t) if !t.trim().is_empty() => t,
            Some(_) => format!("(no {})", labels[i]),
            None => {
                run.out.note(format!("the {} output was not valid JSON and is shown as it came", labels[i]));
                super::text::clean(&cmd.stdout)
            }
        };
        let cut = cut_to_bytes(text.trim_end(), cap.saturating_sub(64), Keep::Head);
        used += cut.text.len();
        run.out.push(Part::new(labels[i], cut.text));
    }
    if run.out.parts.is_empty() {
        return Err(Status::Failed(failures.join("; ")));
    }
    if !failures.is_empty() {
        run.out.status = Status::Partial(failures.join("; "));
    }
    Ok(())
}

// ---- net_dns ---------------------------------------------------------------------------------------------------------

pub fn net_dns(run: &mut Run) -> ProbeResult {
    plain_text_probe(run, "resolvectl reports no resolver state.")
}

// ---- net_listen ------------------------------------------------------------------------------------------------------

/// Programmnamen aus `users:(("firefox",pid=1958,fd=263),("firefox",pid=…))`.
fn listener_processes(users: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = users;
    while let Some(start) = rest.find("((\"").or_else(|| rest.find(",(\"")) {
        let after = &rest[start + 3..];
        let Some(end) = after.find('"') else { break };
        let name = after[..end].to_string();
        if !names.contains(&name) {
            names.push(name);
        }
        rest = &after[end..];
    }
    names
}

/// Port einer Adresse wie `0.0.0.0:22`, `[::]:22` oder `*:22`.
fn port_of(local: &str) -> u32 {
    local.rsplit(':').next().and_then(|p| p.parse().ok()).unwrap_or(0)
}

/// Ab so vielen Sockets eines Programms je Protokoll werden sie zu einer Zeile.
const LISTEN_GROUP: usize = 8;

/// Ein lauschender Socket.
struct Listener {
    proto: String,
    state: String,
    local: String,
    process: String,
}

fn listen_text(text: &str) -> String {
    let mut rows: Vec<Listener> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.len() < 5 {
            continue;
        }
        let process = f.get(6).map(|u| listener_processes(u).join(",")).filter(|p| !p.is_empty()).unwrap_or_else(|| "-".to_string());
        rows.push(Listener { proto: f[0].to_string(), state: f[1].to_string(), local: f[4].to_string(), process });
    }
    rows.sort_by(|a, b| (&a.proto, port_of(&a.local), &a.local).cmp(&(&b.proto, port_of(&b.local), &b.local)));
    let mut table_rows = vec![vec!["proto".to_string(), "state".into(), "local address".into(), "process".into()]];
    let mut grouped: std::collections::BTreeMap<(&str, &str), Vec<&Listener>> = Default::default();
    for r in &rows {
        grouped.entry((r.proto.as_str(), r.process.as_str())).or_default().push(r);
    }
    for r in &rows {
        let group = &grouped[&(r.proto.as_str(), r.process.as_str())];
        if group.len() > LISTEN_GROUP && r.process != "-" {
            if std::ptr::eq(group[0], r) {
                let examples: Vec<&str> = group.iter().take(3).map(|g| g.local.as_str()).collect();
                table_rows.push(vec![r.proto.clone(), r.state.clone(), format!("{} sockets, e.g. {}", group.len(), examples.join(", ")), r.process.clone()]);
            }
            continue;
        }
        table_rows.push(vec![r.proto.clone(), r.state.clone(), r.local.clone(), r.process.clone()]);
    }
    table(&table_rows)
}

pub fn net_listen(run: &mut Run) -> ProbeResult {
    let cap = run.spec.limit_bytes * 4;
    let cmd = single(run, cap, Keep::Head)?;
    let text = cmd.text();
    if text.trim().is_empty() {
        run.out.push(Part::new("", "No listening sockets."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let rendered = listen_text(&text);
    let cut = cut_to_bytes(rendered.trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    if cut.omitted_lines > 0 {
        run.out.note(format!("{} more sockets are not shown", cut.omitted_lines));
    }
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- net_tailscale ---------------------------------------------------------------------------------------------------

fn yes_no(v: &Value, key: &str) -> &'static str {
    match v.get(key).and_then(Value::as_bool) {
        Some(true) => "yes",
        Some(false) => "no",
        None => "?",
    }
}

/// Kurzfassung von `tailscale status --json`: Zustand, Warnungen, der eigene Knoten und die Gegenstellen. Keine Adressen,
/// Schlüssel oder Anmelde-URLs.
fn tailscale_text(doc: &Value) -> String {
    let mut out = String::new();
    out.push_str(&format!("backend: {}  version: {}\n", str_field(doc, "BackendState"), str_field(doc, "Version")));
    let health: Vec<&str> = doc.get("Health").and_then(Value::as_array).map(|h| h.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
    if health.is_empty() {
        out.push_str("health: no warnings\n");
    } else {
        for h in health {
            out.push_str(&format!("health: {}\n", h.chars().take(300).collect::<String>()));
        }
    }
    if let Some(me) = doc.get("Self") {
        out.push_str(&format!(
            "self: {} ({}) online={} relay={} os={}\n",
            str_field(me, "HostName"),
            str_field(me, "DNSName").trim_end_matches('.'),
            yes_no(me, "Online"),
            str_field(me, "Relay"),
            str_field(me, "OS")
        ));
    }
    if let Some(peers) = doc.get("Peer").and_then(Value::as_object) {
        let online = peers.values().filter(|p| p.get("Online").and_then(Value::as_bool) == Some(true)).count();
        out.push_str(&format!("peers: {} ({online} online)\n", peers.len()));
        let mut rows = vec![vec!["peer".to_string(), "online".into(), "relay".into(), "last seen".into(), "exit node".into()]];
        let mut sorted: Vec<&Value> = peers.values().collect();
        sorted.sort_by_key(|p| str_field(p, "HostName"));
        for p in sorted {
            let seen = str_field(p, "LastSeen");
            rows.push(vec![
                str_field(p, "HostName"),
                yes_no(p, "Online").to_string(),
                str_field(p, "Relay"),
                if seen.starts_with("0001-") { "-".to_string() } else { seen },
                yes_no(p, "ExitNode").to_string(),
            ]);
        }
        out.push_str(&table(&rows));
    }
    out
}

pub fn net_tailscale(run: &mut Run) -> ProbeResult {
    let cmd = single(run, RAW_LIMIT, Keep::Head)?;
    let raw = cmd.raw_text();
    let Some(doc) = json::document(&raw) else {
        return Err(Status::Failed(if cmd.truncated {
            format!("tailscale status is larger than {} and was cut, so it cannot be summarised", human_bytes(RAW_LIMIT as u64))
        } else {
            "tailscale status returned something that is not JSON".to_string()
        }));
    };
    let text = tailscale_text(&doc);
    run.out.names = tailscale_names(&doc);
    let cut = cut_to_bytes(text.trim_end(), run.spec.limit_bytes.saturating_sub(64), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

/// Namen des eigenen Knotens und der Gegenstellen (`HostName`, `DNSName`) für das Pseudonymisieren.
pub fn tailscale_names(doc: &Value) -> Vec<(Category, String)> {
    let mut names = Vec::new();
    let nodes = doc.get("Self").into_iter().chain(doc.get("Peer").and_then(Value::as_object).into_iter().flat_map(|p| p.values()));
    for node in nodes {
        for field in ["HostName", "DNSName"] {
            let v = str_field(node, field);
            let v = v.trim_end_matches('.');
            if !v.is_empty() {
                names.push((Category::Host, v.to_string()));
            }
        }
    }
    names
}

/// Namen aus der Quelle eines Netz-Mounts: `//server/freigabe` (Rechner, Freigabe mit Rechner), `user@server:/pfad`
/// (Benutzer, Rechner), `server:/pfad` (Rechner).
pub fn mount_source_names(source: &str) -> Vec<(Category, String)> {
    let mut names = Vec::new();
    if let Some(rest) = source.strip_prefix("//") {
        let mut it = rest.split('/');
        if let Some(host) = it.next().filter(|h| !h.is_empty()) {
            names.push((Category::Host, host.to_string()));
        }
        // die Freigabe nur zusammen mit dem Rechner: allein ist ihr Name oft ein Wort (`share`, `public`)
        if let Some(share) = it.next().filter(|s| !s.is_empty()) {
            names.push((Category::Path, format!("//{}/{share}", rest.split('/').next().unwrap_or(""))));
        }
    } else if let Some((remote, _)) = source.split_once(':') {
        let (user, host) = match remote.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, remote),
        };
        if let Some(u) = user.filter(|u| !u.is_empty()) {
            names.push((Category::User, u.to_string()));
        }
        if !host.is_empty() && !host.starts_with('[') {
            names.push((Category::Host, host.to_string()));
        }
    }
    names
}

// ---- net_mounts ------------------------------------------------------------------------------------------------------

/// Namen von Mount-Optionen, deren Wert Zugangsdaten ist.
const CREDENTIAL_OPTIONS: &[&str] =
    &["credentials", "cred", "password", "pass", "passwd", "pwd", "username", "user", "domain", "workgroup", "username_file", "password_file"];

/// Entfernt aus einer Optionsliste (`rw,relatime,username=bob,credentials=/etc/x`) alle Optionen mit Zugangsdaten
/// (`name=wert` mit einem Namen der Liste; `user` ohne Wert ist die fstab-Option „User darf mounten“ und bleibt).
pub fn redact_mount_options(options: &str) -> (String, usize) {
    let mut removed = 0;
    let kept: Vec<&str> = options
        .split(',')
        .filter(|opt| match opt.split_once('=') {
            Some((name, _)) if CREDENTIAL_OPTIONS.contains(&name.trim().to_lowercase().as_str()) => {
                removed += 1;
                false
            }
            _ => true,
        })
        .collect();
    (kept.join(","), removed)
}

fn mounts_rows(filesystems: &[Value], rows: &mut Vec<Vec<String>>, removed: &mut usize, names: &mut Vec<(Category, String)>) {
    for f in filesystems {
        let (options, n) = redact_mount_options(&str_field(f, "options"));
        *removed += n;
        names.extend(mount_source_names(&str_field(f, "source")));
        rows.push(vec![str_field(f, "target"), str_field(f, "fstype"), str_field(f, "source"), options]);
        if let Some(children) = f.get("children").and_then(Value::as_array) {
            mounts_rows(children, rows, removed, names);
        }
    }
}

pub fn net_mounts(run: &mut Run) -> ProbeResult {
    let limit = run.spec.limit_bytes;
    let argv = commands(run)?.remove(0);
    let cmd = run.exec(&argv, RAW_LIMIT, Keep::Head);
    // `findmnt -t …` ohne Treffer: leere Ausgabe, Exit-Code 1
    if is_silent_no_match(&cmd) {
        run.out.push(Part::new("", "No CIFS, NFS or sshfs file systems are mounted."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    if let Some(status) = cmd.problem() {
        return Err(status);
    }
    let raw = cmd.raw_text();
    let Some(doc) = json::document(&raw) else {
        return Err(Status::Failed("findmnt returned something that is not JSON".into()));
    };
    let list = doc.get("filesystems").and_then(Value::as_array).cloned().unwrap_or_default();
    if list.is_empty() {
        run.out.push(Part::new("", "No CIFS, NFS or sshfs file systems are mounted."));
        run.out.status = Status::Empty;
        return Ok(());
    }
    let mut rows = vec![vec!["target".to_string(), "type".into(), "source".into(), "options".into()]];
    let mut removed = 0usize;
    let mut names = Vec::new();
    mounts_rows(&list, &mut rows, &mut removed, &mut names);
    run.out.names = names;
    run.out.redacted("mount_option", removed);
    let cut = cut_to_bytes(table(&rows).trim_end(), limit.saturating_sub(64), Keep::Head);
    run.out.push(Part::new("", cut.text));
    Ok(())
}

// ---- net_reach -------------------------------------------------------------------------------------------------------

pub fn net_reach(run: &mut Run) -> ProbeResult {
    plain_text_probe(run, "ping printed nothing.")
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn listener_names_come_out_of_the_users_field() {
        assert_eq!(listener_processes(r#"users:(("firefox",pid=1958,fd=263))"#), ["firefox"]);
        assert_eq!(listener_processes(r#"users:(("nginx",pid=1,fd=6),("nginx",pid=2,fd=6),("worker",pid=3,fd=1))"#), ["nginx", "worker"]);
        assert!(listener_processes("").is_empty());
        assert_eq!(port_of("0.0.0.0:22"), 22);
        assert_eq!(port_of("[::]:443"), 443);
        assert_eq!(port_of("*:5353"), 5353);
        assert_eq!(port_of("weird"), 0);
    }

    #[test]
    fn mount_sources_and_tailscale_nodes_name_hosts_users_and_shares() {
        use Category::*;
        let n = |v: &[(Category, &str)]| v.iter().map(|(c, s)| (*c, s.to_string())).collect::<Vec<_>>();
        assert_eq!(mount_source_names("//nas/Daniel"), n(&[(Host, "nas"), (Path, "//nas/Daniel")]));
        assert_eq!(mount_source_names("dkurd@server.example.com:/srv"), n(&[(User, "dkurd"), (Host, "server.example.com")]));
        assert_eq!(mount_source_names("vault:/export"), n(&[(Host, "vault")]));
        assert_eq!(mount_source_names("/dev/sda1"), n(&[]));
        let doc: Value = serde_json::from_str(
            r#"{"Self":{"HostName":"werkbank","DNSName":"werkbank.tail4f2a.ts.net."},"Peer":{"k1":{"HostName":"Daniel's iPad","DNSName":"daniels-ipad.tail4f2a.ts.net."}}}"#,
        )
        .unwrap();
        assert_eq!(
            tailscale_names(&doc),
            n(&[(Host, "werkbank"), (Host, "werkbank.tail4f2a.ts.net"), (Host, "Daniel's iPad"), (Host, "daniels-ipad.tail4f2a.ts.net")])
        );
    }

    #[test]
    fn mount_options_lose_their_credentials_only() {
        let (o, n) = redact_mount_options("rw,relatime,vers=3.1.1,username=bob,credentials=/etc/smb.cred,Password=hunter2,addr=192.0.2.5,user");
        assert_eq!(o, "rw,relatime,vers=3.1.1,addr=192.0.2.5,user");
        assert_eq!(n, 3);
        assert_eq!(redact_mount_options("rw"), ("rw".to_string(), 0));
        assert_eq!(redact_mount_options(""), (String::new(), 0));
        let (o, n) = redact_mount_options("domain=CORP,pass=x,pwd=y,passwd=z,cred=/c,workgroup=W,username_file=/f");
        assert_eq!((o.as_str(), n), ("", 7));
    }
}
