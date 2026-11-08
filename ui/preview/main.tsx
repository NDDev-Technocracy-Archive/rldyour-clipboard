// Isolated browser qualification surface. It never opens a user's clipboard or archive.
import { useState } from "react";
import { createRoot } from "react-dom/client";
import { I18nextProvider } from "react-i18next";
import type { DesktopEntry } from "../generated/api/types.gen";
import { ClipboardWindow, type HistorySource } from "../src/ClipboardWindow";
import { type Language, localization, resources } from "../src/localization";

const parameters = new URL(location.href).searchParams;
const requestedLanguage = parameters.get("language") ?? "ru";
const initialLanguage: Language =
  requestedLanguage in resources ? (requestedLanguage as Language) : "ru";
const i18n = await localization(initialLanguage);
document.documentElement.dataset.theme = parameters.get("theme") ?? "light";
const entries: DesktopEntry[] = [
  {
    key: "9223372036854775807",
    id: Number("9223372036854775807"),
    kind: "text",
    mimes: ["text/plain"],
    bytes: 32,
    preview: "Сохранённый фрагмент для нового проекта",
    pinned: true,
    thumb: false,
    at: 1794133800,
  },
  {
    key: "2",
    id: 2,
    kind: "image",
    mimes: ["image/png"],
    bytes: 4800000,
    preview: null,
    width: 1600,
    height: 1000,
    pinned: false,
    thumb: true,
    at: 1794133740,
  },
  {
    key: "3",
    id: 3,
    kind: "files",
    mimes: ["text/uri-list"],
    bytes: 128,
    preview: "Фотография.png · Макет проекта.pdf",
    pinned: false,
    thumb: false,
    at: 1794133680,
  },
  {
    key: "4",
    id: 4,
    kind: "text",
    mimes: ["text/plain"],
    bytes: 160,
    preview:
      "A deliberately long copied paragraph — محفوظ في الحافظة — 项目的备注 — that wraps without covering the favorite control.",
    pinned: false,
    thumb: false,
    at: 1794133620,
  },
  {
    key: "5",
    id: 5,
    kind: "link",
    mimes: ["text/plain"],
    bytes: 22,
    preview: "https://example.invalid/clipboard",
    pinned: true,
    thumb: false,
    at: 1794133560,
  },
  {
    key: "6",
    id: 6,
    kind: "text",
    mimes: ["text/plain"],
    bytes: 64,
    preview: "<script>window.clipboardScriptExecuted = true</script>",
    pinned: false,
    thumb: false,
    at: 1794133500,
  },
];
const photo = `data:image/svg+xml,${encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80"><rect width="80" height="80" fill="#d8e8e2"/><circle cx="59" cy="21" r="9" fill="#f9c958"/><path d="M0 70 27 28 54 70M30 80 59 43 80 70V80" fill="#366960"/></svg>')}`;
let rows = parameters.has("empty")
  ? []
  : entries.map((entry) => ({ ...entry }));
const source: HistorySource = async (request, signal) => {
  if (signal.aborted) throw new DOMException("Aborted", "AbortError");
  if (parameters.has("failure"))
    throw new Error("Synthetic unavailable archive");
  if (request.action === "favorite") {
    rows = rows.map((entry) =>
      entry.key === request.key ? { ...entry, pinned: request.pinned } : entry,
    );
    return { result: "changed" };
  }
  if (request.action === "restore") {
    document.documentElement.dataset.restoredKey = request.key;
    return { result: "restored" };
  }
  const filtered = rows.filter(
    (entry) =>
      (request.view !== "favorites" || entry.pinned) &&
      (entry.preview ?? "")
        .toLocaleLowerCase()
        .includes(request.query.toLocaleLowerCase()),
  );
  const offset = request.before
    ? filtered.findIndex((entry) => entry.key === request.before) + 1
    : 0;
  const page = filtered.slice(offset, offset + 4);
  return {
    result: "page",
    items: page,
    more: filtered.length > offset + page.length,
  };
};

function Preview() {
  const [language, setLanguage] = useState<Language>(initialLanguage);
  const [closed, setClosed] = useState(false);
  return (
    <I18nextProvider i18n={i18n}>
      {!closed && (
        <ClipboardWindow
          source={source}
          language={language}
          changeLanguage={(value) => {
            void i18n.changeLanguage(value).then(() => setLanguage(value));
          }}
          close={() => setClosed(true)}
          thumbnail={(entry) => (entry.kind === "image" ? photo : undefined)}
        />
      )}
    </I18nextProvider>
  );
}
const root = document.getElementById("root");
if (!root) throw new Error("Preview root is absent");
createRoot(root).render(<Preview />);
