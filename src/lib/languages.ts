export const LANGUAGES: { code: string; label: string }[] = [
  { code: "en", label: "English" },
  { code: "vi", label: "Vietnamese" },
  { code: "zh", label: "Chinese" },
  { code: "ja", label: "Japanese" },
  { code: "ko", label: "Korean" },
  { code: "fr", label: "French" },
  { code: "de", label: "German" },
  { code: "es", label: "Spanish" },
  { code: "pt", label: "Portuguese" },
  { code: "ru", label: "Russian" },
  { code: "it", label: "Italian" },
  { code: "nl", label: "Dutch" },
  { code: "pl", label: "Polish" },
  { code: "tr", label: "Turkish" },
  { code: "ar", label: "Arabic" },
  { code: "hi", label: "Hindi" },
  { code: "bn", label: "Bengali" },
  { code: "th", label: "Thai" },
  { code: "id", label: "Indonesian" },
  { code: "ms", label: "Malay" },
  { code: "tl", label: "Filipino" },
  { code: "sv", label: "Swedish" },
  { code: "da", label: "Danish" },
  { code: "no", label: "Norwegian" },
  { code: "fi", label: "Finnish" },
  { code: "cs", label: "Czech" },
  { code: "sk", label: "Slovak" },
  { code: "el", label: "Greek" },
  { code: "he", label: "Hebrew" },
  { code: "uk", label: "Ukrainian" },
  { code: "ro", label: "Romanian" },
  { code: "hu", label: "Hungarian" },
  { code: "bg", label: "Bulgarian" },
  { code: "hr", label: "Croatian" },
  { code: "sr", label: "Serbian" },
  { code: "fa", label: "Persian" },
  { code: "ur", label: "Urdu" },
];

export function languageName(code: string): string {
  return LANGUAGES.find((l) => l.code === code)?.label ?? code;
}

/** Only valid as a "from" language — the model detects it itself. */
export const AUTO_DETECT = { code: "auto", label: "Auto-detect" };
