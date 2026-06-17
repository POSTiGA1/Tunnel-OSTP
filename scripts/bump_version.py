import os
import sys
import json
import re

def main():
    if len(sys.argv) < 2:
        print("Usage: python bump_version.py <version>")
        sys.exit(1)
        
    version = sys.argv[1]
    if version.startswith("v"):
        version = version[1:]
        
    print(f"Bumping version to {version}")
    
    cargo_paths = ["Cargo.toml", "ostp-gui/src-tauri/Cargo.toml"]
    for cp in cargo_paths:
        if os.path.exists(cp):
            content = open(cp, "r", encoding="utf-8").read()
            content = re.sub(r'(?m)^version = ".*"$', f'version = "{version}"', content, count=1)
            open(cp, "w", encoding="utf-8").write(content)
            print(f"Updated {cp}")
        
    # 2. Update ostp-gui/package.json
    pkg_path = "ostp-gui/package.json"
    if os.path.exists(pkg_path):
        with open(pkg_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        data["version"] = version
        with open(pkg_path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
        print(f"Updated {pkg_path}")
        
    # 3. Update ostp-gui/src-tauri/tauri.conf.json
    tauri_path = "ostp-gui/src-tauri/tauri.conf.json"
    if os.path.exists(tauri_path):
        with open(tauri_path, "r", encoding="utf-8") as f:
            data = json.load(f)
        data["version"] = version
        with open(tauri_path, "w", encoding="utf-8") as f:
            json.dump(data, f, indent=2)
        print(f"Updated {tauri_path}")

if __name__ == "__main__":
    main()
