import { describe, expect, test } from 'bun:test';
import { createCompositionGuard, shouldSendOnEnter } from '../src/lib/composer';

describe('assistant composer', () => {
  test('sends on plain Enter and keeps Shift+Enter as a newline', () => {
    const guard = createCompositionGuard(() => 1000);
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: false, isComposing: false }, guard)).toBe(true);
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: true, isComposing: false }, guard)).toBe(false);
  });

  test('ignores IME Enter, including the final Enter after compositionend', () => {
    let time = 1000;
    const guard = createCompositionGuard(() => time);
    guard.start();
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: false, isComposing: true }, guard)).toBe(false);
    guard.end();
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: false, isComposing: false }, guard)).toBe(false);
    time += 101;
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: false, isComposing: false }, guard)).toBe(true);
    expect(shouldSendOnEnter({ key: 'Enter', shiftKey: false, isComposing: false, keyCode: 229 }, guard)).toBe(false);
  });
});
