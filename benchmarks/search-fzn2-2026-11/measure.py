#!/usr/bin/env python3
"""Run one experiment, preserving its command and aggregate child CPU usage.

Example: python measure.py resource.json python tools/arena.py ...
maxrss is the largest child process, not concurrent aggregate RSS (Linux KiB).
"""
import datetime
import json
import os
from pathlib import Path
import platform
import resource
import subprocess
import sys
import time


def main():
    output, *command = sys.argv[1:]
    start = time.perf_counter()
    completed = subprocess.run(command)
    usage = resource.getrusage(resource.RUSAGE_CHILDREN)
    Path(output).write_text(json.dumps({
        "command": command,
        "cwd": os.getcwd(),
        "finished_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "returncode": completed.returncode,
        "wall_s": time.perf_counter() - start,
        "child_user_s": usage.ru_utime,
        "child_system_s": usage.ru_stime,
        "max_child_rss_kib": usage.ru_maxrss,
        "platform": platform.platform(),
        "affinity": sorted(os.sched_getaffinity(0)),
        "cpu_quota": Path("/sys/fs/cgroup/cpu.max").read_text().strip(),
    }, indent=2) + "\n")
    return completed.returncode


if __name__ == "__main__":
    sys.exit(main())
