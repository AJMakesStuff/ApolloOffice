import json, os, re, subprocess, sys

a = sys.argv[1:]
sysp = a[a.index("--system-prompt") + 1]
p = sys.stdin.read()
mc = a[a.index("--mcp-config") + 1] if "--mcp-config" in a else None
open(os.environ["FAKE_GH_DIR"] + "/claude.log", "a").write(sysp.split("\n")[0][:60] + "\n")


def emit(v):
    print(json.dumps(v), flush=True)


def ids():
    m = re.search(r"Use exactly these ids as JSON keys, in this order: (.*)$", p.strip())
    return [x.strip() for x in m.group(1).split(",")] if m else []


class M:
    def __init__(self):
        c = json.load(open(mc))["mcpServers"]["office"]
        self.p = subprocess.Popen([c["command"]] + c["args"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
        self.n = 0
        self.rq("initialize", {"protocolVersion": "2025-06-18"})

    def rq(self, m, pa):
        self.n += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.n, "method": m, "params": pa}) + "\n")
        self.p.stdin.flush()
        return json.loads(self.p.stdout.readline())

    def sh(self, c):
        r = self.rq("tools/call", {"name": "sh", "arguments": {"cmd": c}})
        t = r["result"]["content"][0]["text"]
        emit({"type": "assistant", "message": {"content": [{"type": "tool_use", "name": "mcp__office__sh", "input": {"cmd": c}}]}})
        return t


emit({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed", "resetsAt": 0}})
f = sysp.split("\n")[0]
if f.startswith("You are Scout"):
    r = {i: {"pick": True, "cx": "simple", "why": "clear bug"} for i in ids()}
elif f.startswith("You are Guard"):
    r = {i: {"inj": False, "why": "normal report"} for i in ids()}
elif f.startswith("You are Vet"):
    r = {"ok": True, "cx": "simple", "why": "clear bug with steps", "hint": "add.sh"}
elif f.startswith("You are Repro"):
    m = M()
    o = m.sh("bash -c 'source add.sh; add 2 3'")
    r = {"ok": "-1" in o, "how": "add 2 3 printed " + o.split("\n")[0], "where": "add.sh:1", "test": "t/add_test.sh"}
elif f.startswith("You are Fixer"):
    m = M()
    m.sh("printf '%s\\n' 'source ./add.sh' 'if [ \"$(add 2 3)\" = 5 ]; then echo \"ok 1 - adds two numbers\"; else echo \"not ok 1 - adds two numbers\"; exit 1; fi' > t/add_test.sh && git add t/add_test.sh && git commit -q -m 'test: add sums two numbers'")
    m.sh("sed -i.bak 's/\\$1 - \\$2/$1 + $2/' add.sh && rm -f add.sh.bak && git add add.sh && git commit -q -m 'fix: add sums instead of subtracting'")
    r = {"test_cmd": "bash t/add_test.sh", "tests": ["adds two numbers"], "title": "Make add sum its arguments", "cause": "add subtracted the second argument.", "fix": "Use + in add.", "untouched": ""}
elif f.startswith("You are Reviewer"):
    r = {"ok": True}
elif f.startswith("You are Watcher"):
    r = {"act": False, "change": "", "draft": ""}
else:
    r = {}
emit({"type": "result", "subtype": "success", "is_error": False, "result": "```json\n" + json.dumps(r) + "\n```", "usage": {"input_tokens": 10, "output_tokens": 5}})
