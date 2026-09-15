"""一次性批量导入：音频素材/ → 音效片段库，角色扩充/ → 角色图库。
直接读写 ~/.kimi-rabbit-widget/config.json（导入前需关闭挂件进程）。"""
import json
import shutil
import time
from pathlib import Path

HOME = Path.home()
APP_DIR = HOME / ".kimi-rabbit-widget"
CFG = APP_DIR / "config.json"
PROJ = Path(r"C:/Users/15214/Desktop/project/kimi桌宠")

AUDIO_EXTS = {".mp3", ".wav", ".ogg", ".m4a", ".flac"}
IMG_EXTS = {".png", ".jpg", ".jpeg", ".gif", ".webp"}


def import_dir(src_dir: Path, lib_dir: Path, key: str, exts: set, cfg: dict, base_ts: int) -> int:
    lib_dir.mkdir(parents=True, exist_ok=True)
    entries = cfg.setdefault(key, [])
    existing_files = {e.get("file") for e in entries}
    count = 0
    for i, f in enumerate(sorted(src_dir.iterdir())):
        if f.suffix.lower() not in exts:
            continue
        entry_id = str(base_ts + i)
        dest_name = f"{entry_id}{f.suffix.lower()}"
        if dest_name in existing_files:
            continue
        shutil.copy2(f, lib_dir / dest_name)
        entries.append({"id": entry_id, "name": f.stem, "file": dest_name})
        count += 1
    return count


def main():
    cfg = json.loads(CFG.read_text(encoding="utf-8")) if CFG.exists() else {}
    base = int(time.time() * 1000)
    na = import_dir(PROJ / "音频素材", APP_DIR / "audio", "audio_fragments", AUDIO_EXTS, cfg, base)
    nr = import_dir(PROJ / "角色扩充", APP_DIR / "roles", "roles", IMG_EXTS, cfg, base + 100000)
    CFG.write_text(json.dumps(cfg, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"音频片段 +{na}，角色图 +{nr}")


if __name__ == "__main__":
    main()
