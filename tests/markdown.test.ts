import { describe, expect, test } from 'bun:test';
import { renderMarkdown } from '../src/lib/markdown';

describe('assistant markdown', () => {
  test('renders common formatting and safe links', () => {
    const html = renderMarkdown('# 見出し\n\n**重要** と `code`\n\n- **一つ**\n- 二つ\n\n1. 最初\n2. 次\n\n```rust\nlet value = 1;\n```\n\n| A | B |\n| --- | --- |\n| x | y |\n\n[公式](https://example.com)');
    expect(html).toContain('<h1>見出し</h1>');
    expect(html).toContain('<strong>重要</strong>');
    expect(html).toContain('<code>code</code>');
    expect(html).toContain('<ul>');
    expect(html).toContain('<strong>一つ</strong>');
    expect(html).toContain('<ol>');
    expect(html).toContain('<pre><code>let value = 1;');
    expect(html).toContain('<table>');
    expect(html).toContain('href="https://example.com"');
  });

  test('escapes raw HTML and rejects unsafe links and remote images', () => {
    const html = renderMarkdown('<img src=x onerror=alert(1)>\n\n[危険](javascript:alert%281%29) ![画像](https://example.com/a.png)');
    expect(html).toContain('&lt;img src=x onerror=alert(1)&gt;');
    expect(html).not.toContain('<img');
    expect(html).not.toContain('javascript:');
    expect(html).toContain('画像');
    expect(html).not.toContain('<a');
  });
});
