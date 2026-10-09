# Warning gate fixture

Schema added to a copy of the module skeleton by
`tests/generated_module_is_warning_free.rs`. Each file exercises generated code that
once compiled with a warning:

- `hooks/example_state.hook.yaml`: a snake_case hook name, so state-machine types must
  still come out in UpperCamelCase, and transitions that are all wildcards, so there is
  no unreachable fallback arm. It declares no triggers, so the trigger registry is empty.
- `models/gate_ledger.model.yaml`: a model with no generated routes, so its service
  field is read by nothing generated.
- `hooks/gate_ledger.hook.yaml`: a custom `emit:` the generator can only leave as a
  placeholder, so the event publisher is unused.

Add a file here whenever a new warning is fixed in a template.
