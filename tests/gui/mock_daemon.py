#!/usr/bin/env python3
# Copyright (C) 2026 Daniel Kurdoghlian
# Pushing Pixels, Vienna <https://pushingpixels.at>
# SPDX-License-Identifier: GPL-3.0-or-later
"""Mock-Daemon für das Einstellungsfenster: spricht das v2-Protokoll aus SPEC-v2.md mit Beispieldaten.

Aufruf: mock_daemon.py SOCKET LOGFILE [HTTP_PORT]
Jede set_config-Anfrage wird als JSON-Zeile in LOGFILE protokolliert. Ein kleiner HTTP-Server liefert eine
Testdatei für den Modell-Download (langsam, damit der Fortschritt sichtbar ist).
"""
import hashlib, ipaddress, json, os, re, socket, sys, threading, time, http.server, socketserver, urllib.parse

sock_path, log_path = sys.argv[1], sys.argv[2]
http_port = int(sys.argv[3]) if len(sys.argv) > 3 else 18765

BLOB = (b"dictate-test-model" * 1000) * 700  # ca. 12,6 MB
BLOB_SHA = hashlib.sha256(BLOB).hexdigest()

config = {
    "model": "~/.local/share/dictate/models/ggml-large-v3-turbo-q5_0.bin",
    "vad_model": "~/.local/share/dictate/models/ggml-silero-v5.1.2.bin",
    "language": "de", "source": "", "max_gain_db": 30, "threads": 4, "step_ms": 250, "commit_silence_ms": 700,
    "max_phrase_ms": 15000, "idle_unload_s": 600, "final_beam_size": 1, "max_listen_s": 120,
    "initial_prompt": "", "blocklist": ["Vielen Dank.", "Thanks for watching!"], "support_url": "https://example.org/donate",
    "features": {"voice_commands": True, "vocabulary": True, "history": True, "profiles": True, "ai": True, "actions": True},
    "overlay": {"position": "bottom", "max_width": 720, "transparency": True, "opacity": 0.8, "bottom_margin": 48},
    "typing": {"delay_ms": 12, "trailing_space": True, "output": "paste", "keyboard_only_app_ids": []},
    "history": {"max_entries": 200},
    "vocabulary": {"terms": ["PostgreSQL", "AwesomeWM", "chezmoi"], "replacements": {"post gress": "PostgreSQL"}},
    "profiles": [
        {"name": "terminal", "match_class": ["Alacritty", "kitty", "XTerm"], "punctuation": "strip",
         "capitalize_first": False, "trailing_space": False, "ai_mode": "", "language": ""},
        {"name": "mail", "match_class": ["thunderbird"], "punctuation": "keep",
         "capitalize_first": True, "trailing_space": True, "ai_mode": "email", "language": "en"},
    ],
    # `model` leer = Vorgabemodell des Anbieters (wie `AiCfg::effective_model`)
    "ai": {"preset": "ollama", "provider": "ollama", "endpoint": "", "model": "",
           "api_key_file": "", "timeout_ms": 4000, "default_mode": "clean", "formality": "auto",
           "mode_models": {"to_de": "claude-sonnet-5-5"},
           "modes": {"clean": "Fix punctuation, capitalization and obvious recognition errors. Keep wording and language. Output only the text.",
                     "email": "Rewrite as a short, polite e-mail in the same language. Output only the text.",
                     "to_en": "Translate to English. Output only the translation.",
                     "to_de": "Translate to German. Output only the translation.",
                     "prompt": "Rewrite the rough prompt as a clear, structured prompt in the same language. Do not carry it out."}},
    # Textaktionen wie `config::ActionsCfg`. Der echte Daemon liefert jede eingebaute Aktion; hier fehlen `explain_code`
    # und `refine_prompt` bewusst wie bei einem älteren Daemon (das Fenster ergänzt die Vorgabe beim Laden, smoke.sh
    # prüft, dass die Seite trotzdem ohne Änderung öffnet), `explain` hat einen eigenen Prompt.
    "actions": {"model": "", "timeout_ms": 30000, "max_chars": 8000, "secrets": "local_only", "language": "",
                "include_ai_modes": True,
                "terminal_classes": ["Alacritty", "kitty", "XTerm", "URxvt", "st-256color", "org.wezfurlong.wezterm", "foot"],
                "prompts": {
                    "explain": "Explain the text in two or three short sentences for a technically literate reader.",
                    "translate": "Translate the text: if it is German, into English; otherwise into German. Write idiomatic, grammatically flawless text as a native speaker would, and keep the meaning, tone and register; in German, choose \"du\" or \"Sie\" from the tone of the source and use it throughout. Keep names, product names, code, URLs, file names and numbers unchanged, and keep the line breaks. Output only the translation: no quotes, notes or explanations.",
                    "rephrase": "Rephrase the text so it reads clearer and more natural. Keep the meaning, language, tone and roughly the length; fix grammar, spelling and punctuation. Keep names, code, URLs and numbers unchanged, and keep the line breaks. Output only the rephrased text: no quotes, notes or explanations.",
                    "analyze_error": "Analyze the error message or log output. Say briefly what went wrong, the most likely cause and how to fix it. To gather more information, suggest only read-only commands that inspect the system (status, logs, versions, listing files or settings); never suggest commands that change, delete, install or restart anything, describe such fixes in words instead. Be concise and use a short list."}},
    # Befehlspalette wie `config::PaletteCfg` (alle Felder, `model` leer = Modell von [ai])
    "palette": {"enabled": True, "project_roots": ["/home/git", "~/work"], "project_depth": 2, "frecency": True,
                "web_search_url": "https://duckduckgo.com/?q={query}", "model": "", "timeout_ms": 8000,
                "terminal": [], "project_command": [], "clipboard_command": [], "emoji_command": []},
    # Linux-Diagnose wie `config::DiagnoseCfg` (alle Felder, `model` leer = Vorgabe: Sonnet 5.5 bei Claude, sonst `ai.model`).
    # Eingeschaltet und mit zwei Zielen, damit die Seite gefüllt aussieht; MOCK_DIAGNOSE_OFF=1 zeigt die Vorgaben.
    "diagnose": {"enabled": True, "model": "", "timeout_ms": 90000, "report_kib": 48, "rounds": 3,
                 "temp_log_dir": "",
                 "ping_targets": [{"name": "gateway", "address": "192.0.2.1"}, {"name": "nas", "address": "nas.example.org"}],
                 "pseudonymize": True, "boot_hint": True},
}
ACTION_NAMES = ["explain", "translate", "rephrase", "refine_prompt", "analyze_error", "explain_code"]
# MOCK_CLOUD=1: KI-Server in der Cloud (Anthropic), etwa für die Hinweise auf den Seiten AI und Actions
if os.environ.get("MOCK_CLOUD"):
    config["ai"].update({"preset": "anthropic", "provider": "anthropic", "endpoint": "", "model": "",
                         "api_key_file": "~/.config/anthropic/api-key"})
    config["features"]["ai"] = False
# MOCK_PALETTE_BAD=1: eine Datei mit unzulässigen Werten (Wurzeln, Suchadresse), die das Fenster schon beim Anzeigen rügt
if os.environ.get("MOCK_PALETTE_BAD"):
    config["palette"].update({"project_roots": ["/home/git", "relative/dir", "~user/x"], "web_search_url": "http://duckduckgo.com/?q={query}"})
# MOCK_PALETTE_OFF=1: Palette ausgeschaltet (der Daemon antwortet auf pal_* mit der Meldung der echten Palette)
if os.environ.get("MOCK_PALETTE_OFF"):
    config["palette"]["enabled"] = False
# MOCK_DIAGNOSE_OFF=1: Diagnose ausgeschaltet und alles auf den Vorgaben (so startet ein frisches Fenster)
if os.environ.get("MOCK_DIAGNOSE_OFF"):
    config["diagnose"] = {"enabled": False, "model": "", "timeout_ms": 90000, "report_kib": 48, "rounds": 3,
                          "temp_log_dir": "", "ping_targets": [], "pseudonymize": True, "boot_hint": True}
# MOCK_DIAGNOSE_BAD=1: eine Datei mit unzulässigen Werten (Ordner, Ziele), die das Fenster schon beim Anzeigen rügt
if os.environ.get("MOCK_DIAGNOSE_BAD"):
    config["diagnose"].update({"temp_log_dir": "relative/dir", "pseudonymize": False,
                               "ping_targets": [{"name": "gateway", "address": "192.0.2.1"}, {"name": "-nas", "address": "not a host"},
                                                {"name": "Gateway", "address": "192.0.2.2"}]})
history = [
    {"ts": "2026-10-22T09:14:03+02:00", "app": "Alacritty", "raw": "git status", "text": "git status"},
    {"ts": "2026-10-22T09:41:55+02:00", "app": "thunderbird", "raw": "hallo anna danke für die schnelle antwort", "text": "Hallo Anna, danke für die schnelle Antwort."},
    {"ts": "2026-10-22T10:02:10+02:00", "app": "firefox", "raw": "wie hoch ist der Großglockner", "text": "Wie hoch ist der Großglockner?"},
]
lock = threading.Lock()
subs, meter_conns = [], {}


def send(conn, obj):
    try:
        conn.sendall((json.dumps(obj) + "\n").encode())
        return True
    except OSError:
        return False


def log(entry):
    with open(log_path, "a") as f:
        f.write(json.dumps(entry) + "\n")


def argv_error(argv):
    """Wie `PaletteCfg::check_argv`: (Index oder None, Meldung) oder None."""
    if len(argv) > 32:
        return None, f"at most 32 entries, got {len(argv)}"
    for i, a in enumerate(argv):
        if not a:
            return i, "must not be empty"
        if any(ord(ch) < 32 or 127 <= ord(ch) < 160 for ch in a):
            return i, "must not contain control characters"
        if len(a.encode()) > 1024:
            return i, "at most 1024 bytes"
    if argv:
        first = argv[0]
        if first.startswith("-"):
            return 0, "must be a program, not an option"
        if "/" in first and not first.startswith("/"):
            return 0, f'must be a program name or an absolute path, got "{first}"'
        if "/" not in first and any(ch.isspace() for ch in first):
            return 0, "must be one program name; put each argument in its own entry"
        if first.strip() != first:
            return 0, "must not start or end with a space"
    return None


def search_url_error(url):
    """Wie `PaletteCfg::check_search_url`: https, genau ein {query}, nicht im Schema, Host oder Port."""
    if not url.startswith("https://"):
        return "must start with https://"
    n = url.count("{query}")
    if n != 1:
        return f"must contain {{query}} exactly once, found {n}"
    a, b = (urllib.parse.urlsplit(url.replace("{query}", w)) for w in ("a", "bb"))
    if (a.scheme, a.netloc) != (b.scheme, b.netloc):
        return "{query} must be in the path, the query or the fragment, not in the host"
    if not a.hostname:
        return "needs a host name"
    return None


def validate_palette(c):
    """Dieselben Felder und Meldungen wie `Config::validate` und `Config::validate_new` für [palette]."""
    p = c.get("palette", {})
    if not 1 <= p.get("project_depth", 2) <= 4:
        return f"palette.project_depth: must be between 1 and 4, got {p['project_depth']}", "palette.project_depth"
    roots = p.get("project_roots", [])
    if len(roots) > 32:
        return f"palette.project_roots: at most 32 roots, got {len(roots)}", "palette.project_roots"
    for i, r in enumerate(roots):
        msg = None
        if not r.strip():
            msg = "must not be empty"
        elif not (r.startswith("/") or r == "~" or r.startswith("~/")):
            msg = f'must be an absolute path or start with ~/, got "{r}"'
        if msg:
            return f"palette.project_roots[{i}]: {msg}", f"palette.project_roots[{i}]"
    msg = search_url_error(p.get("web_search_url", ""))
    if msg:
        return f"palette.web_search_url: {msg}", "palette.web_search_url"
    if not 1000 <= p.get("timeout_ms", 8000) <= 60000:
        return f"palette.timeout_ms: must be between 1000 and 60000, got {p['timeout_ms']}", "palette.timeout_ms"
    for name in ("terminal", "project_command", "clipboard_command", "emoji_command"):
        found = argv_error(p.get(name, []))
        if found:
            idx, msg = found
            where = f"palette.{name}" if idx is None else f"palette.{name}[{idx}]"
            return f"{where}: {msg}", where
    model = p.get("model", "").strip()
    ai = c["ai"]
    if p.get("enabled") and model and ai["provider"] == "anthropic" and not ai.get("endpoint") and not model.lower().startswith("claude-"):
        return f"palette.model: \"{model}\" is not a Claude model: the Anthropic API only serves claude-… models", "palette.model"
    return None


def ping_name_error(name):
    """Wie `PingTargetEntry::check_name`: Buchstabe oder Ziffer vorn, dann Buchstaben, Ziffern und _ . : -, höchstens 64."""
    if not name:
        return "must not be empty"
    if len(name) > 64:
        return f"must be at most 64 characters, got {len(name)}"
    if not re.match(r"[A-Za-z0-9]", name):
        return "must start with a letter or digit"
    bad = re.search(r"[^A-Za-z0-9_.:-]", name)
    if bad:
        return f"may only contain letters, digits and _ . : -, found {bad.group(0)!r}"
    return None


def ping_address_error(address):
    """Wie `PingTargetEntry::check_address`: IP-Adresse oder Hostname nach RFC 1123."""
    try:
        ipaddress.ip_address(address)
        return None
    except ValueError:
        pass
    host = address[:-1] if address.endswith(".") else address
    if not host:
        return "must not be empty"
    if len(host) > 253:
        return f"must be at most 253 characters, got {len(host)}"
    for label in host.split("."):
        if not (1 <= len(label) <= 63 and re.fullmatch(r"[A-Za-z0-9-]+", label) and not label.startswith("-") and not label.endswith("-")):
            return f'must be an IP address or a host name (letters, digits and -, dot-separated), got "{address}"'
    return None


def validate_diagnose(c):
    """Dieselben Felder und Meldungen wie `Config::validate` und `Config::validate_new` für [diagnose]."""
    d = c.get("diagnose", {})
    if any(ord(ch) < 32 or ord(ch) == 127 for ch in d.get("model", "")):
        return "diagnose.model: must not contain control characters", "diagnose.model"
    for key, lo, hi in (("timeout_ms", 5000, 300000), ("report_kib", 8, 192), ("rounds", 1, 5)):
        if not lo <= d.get(key, lo) <= hi:
            return f"diagnose.{key}: must be between {lo} and {hi}, got {d[key]}", f"diagnose.{key}"
    dirname = d.get("temp_log_dir", "")
    msg = None
    if not dirname.strip():
        msg = None  # leer = kein Temperaturlogger
    elif any(ord(ch) < 32 or ord(ch) == 127 for ch in dirname):
        msg = "must not contain control characters"
    elif not (dirname.startswith("/") or dirname == "~" or dirname.startswith("~/")):
        msg = f'must be an absolute path or start with ~/, got "{dirname}"'
    elif ".." in dirname.split("/"):
        msg = "must not contain .."
    if msg:
        return f"diagnose.temp_log_dir: {msg}", "diagnose.temp_log_dir"
    targets = d.get("ping_targets", [])
    if len(targets) > 16:
        return f"diagnose.ping_targets: at most 16 targets, got {len(targets)}", "diagnose.ping_targets"
    for i, t in enumerate(targets):
        msg = ping_name_error(t["name"])
        if not msg and any(o["name"].lower() == t["name"].lower() for o in targets[:i]):
            msg = f'duplicate target name "{t["name"]}"'
        if msg:
            return f"diagnose.ping_targets[{i}].name: {msg}", f"diagnose.ping_targets[{i}].name"
        msg = ping_address_error(t["address"])
        if msg:
            return f"diagnose.ping_targets[{i}].address: {msg}", f"diagnose.ping_targets[{i}].address"
    model = d.get("model", "").strip()
    ai = c["ai"]
    if d.get("enabled") and model and ai["provider"] == "anthropic" and not ai.get("endpoint") and not model.lower().startswith("claude-"):
        return f"diagnose.model: \"{model}\" is not a Claude model: the Anthropic API only serves claude-… models", "diagnose.model"
    return None


def validate(c):
    if not 0 <= c["overlay"]["opacity"] <= 1:
        return "opacity must be between 0 and 1", "overlay.opacity"
    if c["ai"]["timeout_ms"] > 60000:
        return "timeout_ms must be at most 60000", "ai.timeout_ms"
    # Modell wie `Config::validate_new` (vereinfacht): Pflicht ohne Vorgabe, die Anthropic-API nur mit claude-…
    ai = c["ai"]
    if c["features"].get("ai") or c["features"].get("actions"):
        default = {"anthropic": "claude-haiku-5-5", "ollama": "qwen2.5:7b-instruct"}.get(ai["provider"], "")
        model = ai.get("model", "").strip() or default
        if not model:
            return "ai.model: is required: this server has no default model, enter the name of one of its models", "ai.model"
        if ai["provider"] == "anthropic" and not ai.get("endpoint") and not model.lower().startswith("claude-"):
            return f"ai.model: \"{model}\" is not a Claude model: the Anthropic API only serves claude-… models", "ai.model"
    # Textaktionen: dieselben Felder und Meldungen wie `Config::validate`
    a = c.get("actions", {})
    if not 1000 <= a.get("timeout_ms", 30000) <= 120000:
        return f"actions.timeout_ms: must be between 1000 and 120000, got {a['timeout_ms']}", "actions.timeout_ms"
    if not 100 <= a.get("max_chars", 8000) <= 30000:
        return f"actions.max_chars: must be between 100 and 30000, got {a['max_chars']}", "actions.max_chars"
    if a.get("secrets", "local_only") not in ("local_only", "block"):
        return f"actions.secrets: must be one of local_only, block, got \"{a['secrets']}\"", "actions.secrets"
    for i, t in enumerate(a.get("terminal_classes", [])):
        if not t.strip():
            return f"actions.terminal_classes[{i}]: must not be empty", f"actions.terminal_classes[{i}]"
    for name, text in sorted(a.get("prompts", {}).items()):
        if name not in ACTION_NAMES:
            return f"actions.prompts: unknown action \"{name}\" (known: {', '.join(ACTION_NAMES)})", "actions.prompts"
        if not text.strip():
            return f"actions.prompts.{name}: must not be empty", f"actions.prompts.{name}"
    bad = validate_palette(c) or validate_diagnose(c)
    if bad:
        return bad
    for i, p in enumerate(c["profiles"]):
        if not p["name"].strip():
            return "profile name must not be empty", f"profiles[{i}].name"
    return None


def handle_cmd(conn, req):
    global config
    cmd = req.get("cmd")
    if cmd == "get_config":
        return {"ok": True, "config": config, "path": os.path.expanduser("~/.config/dictate/config.toml"), "schema": 2}
    if cmd == "set_config":
        log({"cmd": "set_config", "config": req["config"]})
        bad = validate(req["config"])
        # was der Mock geantwortet hat, damit Tests (diagnose.sh) die Ablehnung eines Feldes prüfen können
        log({"cmd": "set_config_result", "ok": not bad, "field": bad[1] if bad else ""})
        if bad:
            return {"ok": False, "error": bad[0], "field": bad[1]}
        reload = "model" if any(req["config"][k] != config[k] for k in ("model", "language", "threads", "vad_model")) else "live"
        config = req["config"]
        for s in list(subs):
            send(s, {"event": "overlay", "overlay": config["overlay"]})
        return {"ok": True, "reload": reload}
    if cmd == "reload":
        return {"ok": True}
    if cmd == "list_sources":
        return {"ok": True, "sources": [
            {"name": "alsa_input.pci-0000_00_1f.3.analog-stereo", "description": "Built-in Audio Analog Stereo", "default": True},
            {"name": "alsa_input.usb-Blue_Snowball-00.mono-fallback", "description": "Blue Snowball Mono", "default": False}]}
    if cmd == "list_models":
        return {"ok": True,
                "installed": [{"file": "ggml-large-v3-turbo-q5_0.bin", "size": 574041195}, {"file": "ggml-silero-v5.1.2.bin", "size": 885098}],
                "catalog": [
                    {"id": "large-v3-turbo-q5_0", "url": "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin", "sha256": "", "size": 574041195, "note": "Default, fast and accurate"},
                    {"id": "test-small", "url": f"http://127.0.0.1:{http_port}/ggml-test-small.bin", "sha256": BLOB_SHA, "size": len(BLOB), "note": "Local test file"},
                    {"id": "test-bad-sum", "url": f"http://127.0.0.1:{http_port}/ggml-test-bad.bin", "sha256": "0" * 64, "size": len(BLOB), "note": "Checksum fails on purpose"}]}
    if cmd == "history":
        return {"ok": True, "entries": history[: req.get("limit", 50)] if config["features"]["history"] else []}
    if cmd == "history_clear":
        history.clear()
        return {"ok": True}
    if cmd == "ai_models":
        return {"ok": True, "models": [{"name": "qwen2.5:7b-instruct"}, {"name": "llama3.1:8b"}, {"name": "gemma2:9b"}]}
    if cmd == "ai_test":
        time.sleep(0.5)
        return {"ok": True, "text": "[" + req.get("mode", "") + "] " + req.get("text", "").strip().capitalize() + "."}
    if cmd in ("pal_frecency_clear", "pal_sources_refresh"):
        log({"cmd": cmd})
        if not config["palette"]["enabled"]:
            return {"ok": False, "error": "The command palette is off (Settings › Palette)"}
        time.sleep(0.2)
        return {"ok": True}
    if cmd == "meter":
        return {"ok": True}
    if cmd == "status":
        return {"ok": True, "state": "idle"}
    return {"ok": False, "error": f"unknown command: {cmd}"}


def meter_loop(conn):
    t0 = time.time()
    while meter_conns.get(conn, 0) > time.time():
        t = time.time() - t0
        import math
        db = -45 + 30 * max(0, math.sin(t * 2.3)) * (0.6 + 0.4 * math.sin(t * 7))
        if not send(conn, {"event": "level", "db": round(db, 1), "speech": db > -30}):
            return
        time.sleep(0.1)


def client(conn):
    f = conn.makefile("r")
    try:
        for line in f:
            try:
                req = json.loads(line)
            except ValueError:
                send(conn, {"ok": False, "error": "bad request"})
                continue
            if req.get("cmd") == "subscribe":
                subs.append(conn)
                send(conn, {"event": "state", "state": "idle"})
                send(conn, {"event": "overlay", "overlay": config["overlay"]})
                continue
            if req.get("cmd") == "meter":
                log({"cmd": "meter", "on": req.get("on")})
                if req.get("on"):
                    already = meter_conns.get(conn, 0) > time.time()
                    meter_conns[conn] = time.time() + 60
                    if not already:
                        threading.Thread(target=meter_loop, args=(conn,), daemon=True).start()
                else:
                    meter_conns[conn] = 0
            send(conn, handle_cmd(conn, req))
    finally:
        meter_conns.pop(conn, None)
        if conn in subs:
            subs.remove(conn)
        conn.close()


class H(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        self.send_response(200)
        self.send_header("Content-Length", str(len(BLOB)))
        self.end_headers()
        try:
            for i in range(0, len(BLOB), 262144):
                self.wfile.write(BLOB[i:i + 262144])
                time.sleep(0.08)
        except OSError:
            pass

    def log_message(self, *a):
        pass


class S(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True


threading.Thread(target=S(("127.0.0.1", http_port), H).serve_forever, daemon=True).start()
if os.path.exists(sock_path):
    os.unlink(sock_path)
srv = socket.socket(socket.AF_UNIX)
srv.bind(sock_path)
srv.listen(8)
print("mock ready", sock_path, flush=True)
while True:
    c, _ = srv.accept()
    threading.Thread(target=client, args=(c,), daemon=True).start()
