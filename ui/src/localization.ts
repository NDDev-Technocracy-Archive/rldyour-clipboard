import i18next from "i18next";
import ICU from "i18next-icu";
import ar from "../locales/ar.json" with { type: "json" };
import en from "../locales/en.json" with { type: "json" };
import kk from "../locales/kk.json" with { type: "json" };
import ru from "../locales/ru.json" with { type: "json" };
import zh from "../locales/zh.json" with { type: "json" };

export const resources = { ru, en, kk, ar, zh };
export type Language = keyof typeof resources;
export const languageNames: Record<Language, string> = {
  ru: "Русский",
  en: "English",
  kk: "Қазақша",
  ar: "العربية",
  zh: "简体中文",
};

export async function localization(language: Language) {
  const instance = i18next.createInstance();
  await instance.use(ICU).init({
    lng: language,
    fallbackLng: "ru",
    interpolation: { escapeValue: false },
    resources: Object.fromEntries(
      Object.entries(resources).map(([key, value]) => [
        key,
        { translation: value },
      ]),
    ),
  });
  return instance;
}
