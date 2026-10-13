import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type Translation = { text: string; source: string; target: string };
type Mode = "auto" | "tr_en" | "to_tr";
type Settings = {
  has_key: boolean;
  shortcut: string;
  autostart: boolean;
  shortcut_error: string | null;
};

const $ = <T extends HTMLElement = HTMLElement>(id: string) =>
  document.getElementById(id) as T;

const shell = $("shell");
const q = $<HTMLTextAreaElement>("q");
const dir = $("dir");
const gear = $("gear");
const result = $("result");
const out = $("out");
const meta = $("meta");
const error = $("error");
const settings = $("settings");
const keyInput = $<HTMLInputElement>("key");
const recorder = $<HTMLButtonElement>("shortcut");
const autostart = $<HTMLInputElement>("autostart");
const save = $("save");
const settingsMsg = $("settings-msg");

const DEBOUNCE_MS = 350;
const KEY_HINT = "Ücretsiz anahtar: deepl.com/pro-api";

let seq = 0;
let timer: number | undefined;
let lastQuery = "";
let current: Settings | null = null;
let recording = false;

// Otomatik algılama yanılırsa Tab / rozet tıklamasıyla yön zorlanabilir.
const MODES: Mode[] = ["auto", "tr_en", "to_tr"];
const MODE_TITLES: Record<Mode, string> = {
  auto: "Yön: otomatik (değiştirmek için Tab)",
  tr_en: "Yön: Türkçe → İngilizce (değiştirmek için Tab)",
  to_tr: "Yön: → Türkçe (değiştirmek için Tab)",
};
let mode: Mode = "auto";

function setMode(m: Mode) {
  mode = m;
  dir.classList.toggle("forced", m !== "auto");
  dir.title = MODE_TITLES[m];
}

function cycleMode() {
  setMode(MODES[(MODES.indexOf(mode) + 1) % MODES.length]);
  clearTimeout(timer);
  translate(q.value);
}

// ---------- Pencere boyutu ----------

let lastHeight = 0;
new ResizeObserver(() => {
  const h = Math.ceil(shell.getBoundingClientRect().height);
  if (h !== lastHeight) {
    lastHeight = h;
    invoke("resize", { height: h });
  }
}).observe(shell);

function autoGrow() {
  q.style.height = "auto";
  q.style.height = `${q.scrollHeight}px`;
}

// ---------- Çeviri ----------

function setLoading(on: boolean) {
  shell.classList.toggle("loading", on);
}

function showError(msg: string) {
  result.hidden = true;
  dir.hidden = true;
  error.textContent = msg;
  error.hidden = false;
}

function clearResult() {
  seq++;
  lastQuery = "";
  setMode("auto");
  setLoading(false);
  result.hidden = true;
  error.hidden = true;
  dir.hidden = true;
}

async function translate(raw: string) {
  const text = raw.trim();
  if (!text) return clearResult();
  const query = `${mode}|${text}`;
  if (query === lastQuery) return;

  if (current && !current.has_key) {
    openSettings();
    return showError("Önce bir DeepL API anahtarı gir.");
  }

  const id = ++seq;
  setLoading(true);
  try {
    const r = await invoke<Translation>("translate", { text, mode });
    if (id !== seq) return;
    lastQuery = query;
    out.textContent = r.text;
    dir.textContent = `${r.source} → ${r.target}`;
    dir.hidden = false;
    error.hidden = true;
    result.hidden = false;
    resetMeta();
  } catch (e) {
    if (id !== seq) return;
    lastQuery = "";
    showError(String(e));
  } finally {
    if (id === seq) setLoading(false);
  }
}

function schedule() {
  clearTimeout(timer);
  timer = window.setTimeout(() => translate(q.value), DEBOUNCE_MS);
}

const metaHtml = meta.innerHTML;
function resetMeta() {
  meta.classList.remove("copied");
  meta.innerHTML = metaHtml;
}

async function copyAndHide() {
  if (result.hidden || !out.textContent) return;
  await invoke("copy_text", { text: out.textContent });
  meta.classList.add("copied");
  meta.textContent = "Kopyalandı ✓";
  setTimeout(() => invoke("hide_window"), 380);
}

q.addEventListener("input", () => {
  autoGrow();
  if (!q.value.trim()) {
    clearTimeout(timer);
    clearResult();
    return;
  }
  schedule();
});

q.addEventListener("keydown", (e) => {
  if (e.key === "Tab" && !e.ctrlKey && !e.altKey) {
    e.preventDefault();
    if (q.value.trim()) cycleMode();
    return;
  }
  if (e.key !== "Enter" || e.shiftKey) return;
  e.preventDefault();
  if (e.ctrlKey) return void copyAndHide();
  clearTimeout(timer);
  lastQuery = "";
  translate(q.value);
});

dir.addEventListener("click", () => {
  cycleMode();
  q.focus();
});

out.addEventListener("click", () => {
  // Metin seçiliyse kullanıcı kısmi kopyalamak istiyordur
  if (window.getSelection()?.toString()) return;
  copyAndHide();
});

// ---------- Ayarlar ----------

function setMsg(text: string, kind: "" | "ok" | "err" = "") {
  settingsMsg.textContent = text;
  settingsMsg.className = `settings-msg ${kind}`;
}

function renderSettings(s: Settings) {
  current = s;
  keyInput.value = "";
  keyInput.placeholder = s.has_key ? "•••••••• kayıtlı" : "xxxxxxxx-xxxx-…:fx";
  recorder.textContent = s.shortcut;
  autostart.checked = s.autostart;
  if (s.shortcut_error) setMsg(s.shortcut_error, "err");
  else setMsg(s.has_key ? "" : KEY_HINT);
}

async function loadSettings() {
  renderSettings(await invoke<Settings>("get_settings"));
}

function openSettings() {
  settings.hidden = false;
  gear.classList.add("active");
}

function closeSettings() {
  stopRecording(true);
  settings.hidden = true;
  gear.classList.remove("active");
  q.focus();
}

gear.addEventListener("click", () =>
  settings.hidden ? (openSettings(), keyInput.focus()) : closeSettings(),
);

save.addEventListener("click", async () => {
  try {
    const s = await invoke<Settings>("save_settings", {
      deeplKey: keyInput.value.trim(),
      autostart: autostart.checked,
    });
    renderSettings(s);
    setMsg("Kaydedildi", "ok");
    lastQuery = "";
    if (q.value.trim()) translate(q.value);
  } catch (e) {
    setMsg(String(e), "err");
  }
});

keyInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") save.click();
});

// Kısayol kaydedici

function startRecording() {
  recording = true;
  recorder.classList.add("recording");
  recorder.textContent = "Tuş kombinasyonuna bas…";
  setMsg("İptal için Esc");
  invoke("suspend_shortcut");
}

function stopRecording(restore: boolean) {
  if (!recording) return;
  recording = false;
  recorder.classList.remove("recording");
  if (restore) {
    recorder.textContent = current?.shortcut ?? "";
    setMsg("");
    invoke("resume_shortcut");
  }
}

const MODIFIER_CODES = /^(Control|Alt|Shift|Meta|OS)/;

function comboFrom(e: KeyboardEvent): string | null {
  if (MODIFIER_CODES.test(e.code)) return null;
  const mods: string[] = [];
  if (e.ctrlKey) mods.push("Ctrl");
  if (e.altKey) mods.push("Alt");
  if (e.shiftKey) mods.push("Shift");
  if (e.metaKey) mods.push("Super");
  const key = e.code.replace(/^Key/, "").replace(/^Digit/, "");
  const isFn = /^F\d{1,2}$/.test(key);
  if (!mods.length && !isFn) return null;
  return [...mods, key].join("+");
}

recorder.addEventListener("click", () => {
  if (!recording) startRecording();
});

recorder.addEventListener("keydown", async (e) => {
  if (!recording) return;
  e.preventDefault();
  e.stopPropagation();
  if (e.key === "Escape" && !e.ctrlKey && !e.altKey && !e.shiftKey) {
    return stopRecording(true);
  }
  const combo = comboFrom(e);
  if (!combo) return;
  stopRecording(false);
  try {
    const shortcut = await invoke<string>("set_shortcut", { shortcut: combo });
    if (current) current.shortcut = shortcut;
    recorder.textContent = shortcut;
    setMsg("Kısayol güncellendi", "ok");
  } catch (err) {
    recorder.textContent = current?.shortcut ?? "";
    setMsg(String(err), "err");
    invoke("resume_shortcut");
  }
});

recorder.addEventListener("blur", () => stopRecording(true));

// ---------- Genel klavye / pencere olayları ----------

window.addEventListener("keydown", (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    if (!settings.hidden && current?.has_key) closeSettings();
    else invoke("hide_window");
    return;
  }
  // Tarayıcı kısayollarını (yenile, yazdır, bul…) kapat
  const k = e.key.toLowerCase();
  if (e.key === "F5" || e.key === "F7" || ((e.ctrlKey || e.metaKey) && "rpfgjuh".includes(k) && k.length === 1)) {
    e.preventDefault();
  }
});

window.addEventListener("contextmenu", (e) => {
  const t = e.target as HTMLElement;
  if (!t.closest("textarea, input")) e.preventDefault();
});

listen("shown", () => {
  shell.classList.remove("enter");
  void shell.offsetWidth;
  shell.classList.add("enter");
  resetMeta();
  q.focus();
  q.select();
});

listen("hidden", () => {
  stopRecording(true);
  if (mode !== "auto") {
    // Sonraki açılışta yeniden çevrilsin diye eski sonucu geçersiz say.
    setMode("auto");
    lastQuery = "";
  }
});

listen("open-settings", () => {
  openSettings();
  keyInput.focus();
});

setMode("auto");
loadSettings().then(() => {
  if (current && !current.has_key) openSettings();
});
