import { fireEvent, render, screen, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { describe, expect, it, vi } from 'vitest'
import type { RuleDraft, RulesState } from '../api'
import { callsOf, mockBackend } from '../test/tauri'
import { RuleEditor } from './RuleEditor'
import { Rules } from './Rules'

const rule = (name: string, over: Partial<RuleDraft> = {}): RuleDraft => ({
  name,
  enabled: true,
  watch: ['~/Downloads'],
  when: [{ key: 'ext', value: 'pdf' }],
  action: { kind: 'move', value: '~/Docs/' },
  ...over,
})

const state = (drafts: RuleDraft[]): RulesState => ({
  drafts,
  error: null,
  config_path: '~/.config/tidyflow/config.toml',
  mtime: '123',
})

function editor(draft: RuleDraft, onChange = vi.fn(), onSave = vi.fn()) {
  mockBackend({ preview: () => null })
  return render(
    <RuleEditor
      draft={draft}
      savedName={null}
      isNew={false}
      dirty
      error={null}
      busy={false}
      onChange={onChange}
      onSave={onSave}
      onDiscard={vi.fn()}
      onDelete={vi.fn()}
    />,
  )
}

describe('RuleEditor', () => {
  it('adds the first unused condition type', async () => {
    const onChange = vi.fn()
    editor(rule('r'), onChange)
    await userEvent.click(screen.getByText('+ Adicionar condição'))
    expect(onChange.mock.calls[0][0].when).toEqual([
      { key: 'ext', value: 'pdf' },
      { key: 'name', value: '' },
    ])
  })

  it('does not offer types used elsewhere and hides the button when all are used', () => {
    const keys = ['ext', 'name', 'name_regex', 'mime', 'origin', 'site', 'size_gt', 'size_lt', 'older_than'] as const
    editor(rule('r', { when: keys.map((key) => ({ key, value: 'x' })) }))
    expect(screen.queryByText('+ Adicionar condição')).toBeNull()
    const first = screen.getAllByLabelText('Tipo de condição')[0]
    expect(within(first).getAllByRole('option')).toHaveLength(1)
  })

  it('trash hides the destination; variables are inserted', async () => {
    const onChange = vi.fn()
    const { rerender } = editor(rule('r', { action: { kind: 'move', value: '~/Docs/' } }), onChange)
    await userEvent.click(screen.getByRole('button', { name: '{year}' }))
    expect(onChange.mock.calls.at(-1)?.[0].action).toEqual({ kind: 'move', value: '~/Docs/{year}' })

    await userEvent.click(screen.getByRole('tab', { name: 'Lixeira' }))
    expect(onChange.mock.calls.at(-1)?.[0].action).toEqual({ kind: 'trash' })
    rerender(
      <RuleEditor
        draft={rule('r', { action: { kind: 'trash' } })}
        savedName={null}
        isNew={false}
        dirty
        error={null}
        busy={false}
        onChange={onChange}
        onSave={vi.fn()}
        onDiscard={vi.fn()}
        onDelete={vi.fn()}
      />,
    )
    expect(screen.queryByLabelText('Pasta de destino')).toBeNull()
  })

  it('adds a folder from the picker with ~', async () => {
    const onChange = vi.fn()
    editor(rule('r'), onChange)
    await userEvent.click(screen.getByText('Adicionar pasta'))
    await vi.waitFor(() =>
      expect(onChange.mock.calls.at(-1)?.[0].watch).toEqual(['~/Downloads', '~/Desktop']),
    )
  })
})

describe('Rules', () => {
  it('saves the edited draft with the loaded mtime', async () => {
    const drafts = [rule('Faturas'), rule('Limpeza', { action: { kind: 'trash' } })]
    mockBackend({
      rules_list: () => state(drafts),
      preview: () => null,
      save_rule: ({ draft }) => state([draft as RuleDraft, drafts[1]]),
    })
    render(<Rules version={0} />)
    const name = await screen.findByLabelText('Nome da regra')
    await userEvent.clear(name)
    await userEvent.type(name, 'Faturas 2')
    await userEvent.click(screen.getByText('Salvar regra'))
    const call = callsOf('save_rule')[0].args
    expect(call.mtime).toBe('123')
    expect(call.index).toBe(0)
    expect((call.draft as RuleDraft).name).toBe('Faturas 2')
  })

  it('shows validation errors on the form', async () => {
    mockBackend({
      rules_list: () => state([rule('A')]),
      preview: () => null,
      save_rule: () => {
        throw { kind: 'invalid', message: 'regra "A": regex inválida', line: 3 }
      },
    })
    render(<Rules version={0} />)
    fireEvent.change(await screen.findByLabelText('Valor de Extensão'), { target: { value: 'xml' } })
    await userEvent.click(screen.getByText('Salvar regra'))
    expect(await screen.findByRole('alert')).toHaveTextContent('regex inválida')
  })

  it('toggle pauses the rule', async () => {
    mockBackend({
      rules_list: () => state([rule('A'), rule('B')]),
      preview: () => null,
      set_enabled: () => state([rule('A'), rule('B', { enabled: false })]),
    })
    render(<Rules version={0} />)
    await userEvent.click(await screen.findByRole('switch', { name: 'Pausar B' }))
    expect(callsOf('set_enabled')[0].args).toEqual({ mtime: '123', index: 1, enabled: false })
    expect(await screen.findByText('pdf → mover · pausada')).toBeInTheDocument()
  })

  it('invalid config shows the error instead of the list', async () => {
    mockBackend({
      rules_list: () => ({
        ...state([]),
        error: { kind: 'invalid', message: 'chave desconhecida `foo`', line: 7 },
      }),
    })
    render(<Rules version={0} />)
    expect(await screen.findByRole('alert')).toHaveTextContent('linha 7: chave desconhecida `foo`')
    expect(screen.getByText('Abrir config.toml')).toBeInTheDocument()
  })
})
