import { LocaleLabels, Messages } from './locales';
import type { Locale } from './types';

/// 当前桌面端语言。
export let CurrentLocale: Locale = 'zh-CN';

/// 当前语言文案表。
export let I18n = Messages[CurrentLocale];

/// 判断字符串是否是支持的语言。
/// @param locale 待判断的语言标识。
export function IsLocale(locale: string): locale is Locale {
  return Object.prototype.hasOwnProperty.call(Messages, locale);
}

/// 切换当前语言文案表。
/// @param locale 目标语言。
export function SetI18nLocale(locale: Locale) {
  CurrentLocale = locale;
  I18n = Messages[locale];
}

export { LocaleLabels, Messages };
export type { Locale, MessagesSchema } from './types';
