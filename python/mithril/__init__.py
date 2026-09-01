"""
Mithril: Multi-model orchestration engine.
Combine LLM providers into a single Ollama-compatible API.
"""

import os
import sys
import subprocess
import shutil

__version__ = "0.4.0"
__all__ = ["main", "find_mithril_binary", "__version__"]

def find_mithril_binary() -> str:
    """Locate the mithril executable installed alongside python or in PATH."""
    bin_dir = os.path.dirname(sys.executable)
    # Check Python venv/bin or Scripts dir
    for candidate_name in ["mithril", "mithril.exe"]:
        candidate = os.path.join(bin_dir, candidate_name)
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
        # Check module directory
        module_bin = os.path.join(os.path.dirname(__file__), candidate_name)
        if os.path.isfile(module_bin) and os.access(module_bin, os.X_OK):
            return module_bin

    # Check PATH
    path_bin = shutil.which("mithril")
    if path_bin:
        return path_bin

    return ""

def main():
    """Main entry point for mithril CLI wrapper."""
    binary = find_mithril_binary()
    if not binary:
        sys.stderr.write(
            "Error: mithril binary not found.\n"
            "Please ensure mithril is installed in your Python environment or PATH.\n"
        )
        sys.exit(1)

    args = [binary] + sys.argv[1:]
    try:
        res = subprocess.run(args)
        sys.exit(res.returncode)
    except KeyboardInterrupt:
        sys.exit(130)
