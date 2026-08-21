export function authenticatorCodeError(
  value: string,
  required: boolean,
): { kind: string; message: string } | undefined {
  const code = value.trim();
  if (!code) {
    return required ? { kind: 'required', message: '请输入 6 位验证码' } : undefined;
  }
  return /^\d{6}$/.test(code)
    ? undefined
    : { kind: 'authenticatorCode', message: '验证码应为 6 位数字' };
}
