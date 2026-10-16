#!/usr/bin/env python3
"""Run a GGUF model in llama.cpp (llama-server, CUDA) and log one jmon workload line per request.

  python3 ~/llm_jmon.py [MODEL.gguf] [--requests 30] [--tokens 128]
  # second terminal:
  jmon --watch llama-server --workload ~/jmon-llm.log --window 60   (start jmon FIRST for the model-memory estimate)

Log line: lat_ms=<request> batch=<generated tokens> tok_ms=<gen ms/token> engine=<model> stage_prompt=<ms> stage_gen=<ms>
→ jmon "inf/s" = generated tokens/s (Σbatch / window); tok_ms percentiles compare short and long answers fairly;
  lat_ms percentiles mix answer lengths (an early EOS makes a request short).
"""
import argparse, json, os, signal, subprocess, sys, time, urllib.request

HOME = os.path.expanduser("~")
p = argparse.ArgumentParser()
p.add_argument("model", nargs="?", default=f"{HOME}/models/Qwen3.5-2B-Q4_K_M.gguf")
p.add_argument("--requests", type=int, default=30)
p.add_argument("--tokens", type=int, default=128, help="tokens to generate per request")
p.add_argument("--port", type=int, default=8091)
p.add_argument("--log", default=f"{HOME}/jmon-llm.log")
a = p.parse_args()

engine = os.path.basename(a.model).removesuffix(".gguf")
url = f"http://127.0.0.1:{a.port}"
server = subprocess.Popen(
    [f"{HOME}/llama.cpp/build/bin/llama-server", "-m", a.model, "-ngl", "99", "-c", "2048", "--port", str(a.port)],
    stdout=open(f"{HOME}/llama-server.log", "w"), stderr=subprocess.STDOUT)
print(f"llama-server pid {server.pid} loading {engine} (log: ~/llama-server.log)")

def post(path, body):
    req = urllib.request.Request(url + path, json.dumps(body).encode(), {"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=300) as r:
        return json.load(r)

try:
    for _ in range(240):  # wait up to 120 s for the model to load
        try:
            if urllib.request.urlopen(url + "/health", timeout=2).status == 200:
                break
        except Exception:
            pass
        if server.poll() is not None:
            sys.exit("llama-server exited — see ~/llama-server.log")
        time.sleep(0.5)
    else:
        sys.exit("llama-server not ready after 120 s")

    prompts = ["Explain edge AI in two sentences.", "Write a haiku about a GPU.",
               "List three uses of a Jetson Orin Nano.", "What is quantization in LLMs?"]
    with open(a.log, "w", buffering=1) as log:  # truncate → jmon re-reads from the start
        for i in range(a.requests):
            t0 = time.monotonic()
            r = post("/completion", {"prompt": prompts[i % len(prompts)], "n_predict": a.tokens, "cache_prompt": False})
            lat_ms = (time.monotonic() - t0) * 1e3  # server returns only after generation finished
            t = r.get("timings", {})
            n = int(t.get("predicted_n", 0)) or 1
            tok_ms = t.get("predicted_ms", 0) / n
            log.write(f"ts={time.monotonic():.3f} lat_ms={lat_ms:.1f} batch={n} tok_ms={tok_ms:.2f} engine={engine} "
                      f"stage_prompt={t.get('prompt_ms', 0):.1f} stage_gen={t.get('predicted_ms', 0):.1f}\n")
            print(f"[{i+1}/{a.requests}] {lat_ms:7.0f} ms  {n} tok  {tok_ms:5.1f} ms/tok  {t.get('predicted_per_second', 0):5.1f} tok/s")
finally:
    server.send_signal(signal.SIGINT)
    server.wait(timeout=30)
    print("llama-server stopped")
