"""Check whether Farzin confirms loading a local EvalFile before a match."""
import os
import subprocess


def verify_farzin_network(engine, network):
    if "\n" in network or "\r" in network:
        raise ValueError("network path cannot contain a line break")
    commands = f"uci\nsetoption name EvalFile value {network}\nisready\nquit\n"
    result = subprocess.run([os.path.abspath(engine)], input=commands, text=True,
                            capture_output=True, timeout=30, check=True)
    lines = result.stdout.splitlines()
    if "uciok" not in lines or "readyok" not in lines:
        raise RuntimeError("engine did not complete UCI preflight")
    is_farzin = any(line.startswith("id name Farzin ") for line in lines)
    loaded = "info string experimental network loaded" in lines
    if is_farzin and not loaded:
        raise RuntimeError("Farzin did not confirm loading the requested EvalFile")
    return {"engine_is_farzin": is_farzin, "load_confirmed": is_farzin and loaded}
