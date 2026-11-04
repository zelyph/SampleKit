"""A model's scripts: methods marked ``@sk.script``, run by ``samplekit run``.

A script on a method acts on one sample; above ``@classmethod``, on the
selection. Its keyword parameters are the command's options, typed by the
signature, and its docstring its description.
"""

import inspect

# The types an option is read as, by the name the signature gives.
TYPES = {"int": int, "float": float, "bool": bool, "str": str}
TRUE, FALSE = ("true", "yes", "on", "1"), ("false", "no", "off", "0")


class Script:
    """A model's method marked ``@sk.script``. It still behaves as the method
    it marks: ``steel.classify(15)`` runs as written."""

    def __init__(self, method):
        if isinstance(method, staticmethod):
            raise TypeError(
                "a script acts on a sample or a selection: mark an instance method, or a "
                "class method (@sk.script above @classmethod), not a static method"
            )
        self.method = method
        self.collection = isinstance(method, classmethod)
        function = method.__func__ if self.collection else method
        self.name = getattr(function, "__name__", None)
        self.__doc__ = getattr(function, "__doc__", None)
        self.options = _options_of(function, self.collection)

    def __set_name__(self, owner, name):
        self.name = name

    def __get__(self, instance, owner=None):
        return self.method.__get__(instance, owner)

    @property
    def description(self):
        """The docstring's first paragraph, on one line."""
        return first_paragraph(self.__doc__)


def first_paragraph(doc):
    """A docstring's first paragraph, on one line: a script's or a figure's
    description."""
    text = inspect.cleandoc(doc or "")
    return " ".join(text.split("\n\n")[0].split())


def script(method):
    """Mark a method of a model as a script.

    ``samplekit run NAME`` runs it, and ``r`` in the workbench: a method acts
    on one sample, each selected in turn; placed above ``@classmethod``, it
    receives the selection. Its keyword parameters are the command's options,
    ``--set offset=0.002``, read as their annotation or their default says;
    its docstring is its description. What it changes in the samples is
    shown, and written once the change is accepted; what it returns is shown.

    Args:
        method: The method.

    Returns:
        The method, marked.

    Raises:
        TypeError: The method is a static method, or takes ``*args`` or
            ``**kwargs``.

    Example:
        >>> class Specimen(sk.Sample):
        ...     @sk.script
        ...     def classify(self, limit: float = 20.0):
        ...         "Class the specimen's ductility by its elongation at break."
        ...         self.ductility = "high" if self.elongation.value >= limit else "low"
    """
    return Script(method)


def scripts_of(cls):
    """The scripts a model declares, by name, a subclass's over its base's: a
    subclass that redefines the name as something else takes the script away."""
    found = {}
    for klass in reversed(cls.__mro__):
        for name, value in vars(klass).items():
            if isinstance(value, Script):
                found[name] = value
            elif isinstance(value, classmethod) and isinstance(value.__func__, Script):
                raise TypeError(
                    f"{klass.__name__}.{name}: @sk.script goes above @classmethod, "
                    "not beneath it"
                )
            elif name in found:
                del found[name]
    return found


def described(script):
    """What the model's description says of a script: its name, what it acts
    on, its description and its options."""
    return {
        "name": script.name,
        "acts": "collection" if script.collection else "sample",
        "description": script.description,
        "options": [
            {"name": name, "type": kind, "default": default}
            for name, kind, default, _ in script.options
        ],
    }


def _options_of(function, collection):
    """Each option: its name, its type's name, its default as text or None
    where it is required, and the default itself."""
    parameters = list(inspect.signature(function).parameters.values())
    # `self`, or `cls` and the selection.
    taken = 2 if collection else 1
    if len(parameters) < taken:
        raise TypeError(
            f"{function.__name__}: a script on the selection takes it after cls, "
            f"def {function.__name__}(cls, samples)"
        )
    options = []
    for parameter in parameters[taken:]:
        if parameter.kind in (parameter.VAR_POSITIONAL, parameter.VAR_KEYWORD):
            raise TypeError(
                f"{function.__name__}: a script's options are named parameters, and "
                f"*{parameter.name} names none"
            )
        if parameter.kind == parameter.POSITIONAL_ONLY:
            raise TypeError(
                f"{function.__name__}: '{parameter.name}' is positional only, and an "
                "option is given by its name"
            )
        missing = parameter.default is parameter.empty
        options.append((
            parameter.name,
            _type_name(parameter),
            None if missing else _as_text(parameter.default),
            parameter.default,
        ))
    return options


def _type_name(parameter):
    annotation = parameter.annotation
    if isinstance(annotation, str) and annotation in TYPES:
        return annotation
    for name, kind in TYPES.items():
        if annotation is kind:
            return name
    default = parameter.default
    if default is not parameter.empty and default is not None:
        for name in ("bool", "int", "float", "str"):
            if type(default) is TYPES[name]:
                return name
    return "str"


def _as_text(value):
    if value is None:
        return "None"
    if isinstance(value, bool):
        return "true" if value else "false"
    return str(value)


def arguments(script, given):
    """The keyword arguments `given` — each option's text, by name — read as
    the signature types them. A name the script does not take, a text its
    type refuses, or a required option not given is a ValueError naming it."""
    known = {name: (kind, default) for name, kind, default, _ in script.options}
    unknown = [name for name in given if name not in known]
    if unknown:
        return _refused(
            f"'{script.name}' takes no option '{unknown[0]}'",
            script,
        )
    missing = [name for name, (_, default) in known.items()
               if default is None and name not in given]
    if missing:
        return _refused(
            f"'{script.name}' needs {', '.join(missing)}: --set {missing[0]}=VALUE",
            script,
        )
    read = {}
    for name, text in given.items():
        kind = known[name][0]
        if kind == "bool":
            lowered = text.strip().lower()
            if lowered not in TRUE + FALSE:
                raise ValueError(f"--set {name}={text}: a yes or no, true or false")
            read[name] = lowered in TRUE
        elif kind in ("int", "float"):
            try:
                read[name] = TYPES[kind](text.strip())
            except ValueError:
                raise ValueError(
                    f"--set {name}={text}: {'an integer' if kind == 'int' else 'a number'}"
                ) from None
        else:
            read[name] = text
    return read


def _refused(message, script):
    options = ", ".join(
        name if default is None else f"{name}={default}"
        for name, _, default, _ in script.options
    )
    raise ValueError(f"{message} — its options: {options or 'none'}")
