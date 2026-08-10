import type { Locale } from '../i18n';

export type ConfigTheme = 'light' | 'dark';

export type ConfigStorageType = string;

export interface Config {
  compactRatio: number;
  configKey: string;
  language: Locale;
  path: string;
  storageType: ConfigStorageType;
  theme: ConfigTheme;
}
