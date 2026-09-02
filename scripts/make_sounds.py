"""生成月兔娘按压/回弹音效（轻柔气泡音），纯标准库。"""
import math
import struct
import wave

SR = 44100


def synth(path, f0, f1, dur_ms, bright=0.35):
    """正弦滑音 + 一点泛音，起止包络防爆音。"""
    n = int(SR * dur_ms / 1000)
    frames = bytearray()
    phase = 0.0
    for i in range(n):
        t = i / n
        # 指数滑音更像「气泡」
        f = f0 * (f1 / f0) ** t
        phase += 2 * math.pi * f / SR
        # 包络：快起缓落
        env = min(1.0, t / 0.08) * (1 - t) ** 1.6
        s = math.sin(phase) + bright * math.sin(2 * phase)
        v = int(0.5 * env * s * 32767)
        frames += struct.pack('<h', max(-32767, min(32767, v)))
    with wave.open(path, 'wb') as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(bytes(frames))
    print(path, dur_ms, 'ms')


# 按压：下沉 bloop；回弹：上扬 boop
synth('src/assets/press.wav', 520, 240, 140)
synth('src/assets/release.wav', 260, 560, 160)
