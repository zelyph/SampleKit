import samplekit as sk

specimens = sk.load("samples").sorted("-tensile_strength")
for specimen in specimens:
    ratio = specimen.yield_strength.value / specimen.tensile_strength.value
    print(f"{specimen.name:13} {specimen.tensile_strength}   Rp0.2/Rm = {ratio:.2f}")

print(specimens.filter("elongation > 20").to_csv(columns=["name", "young_modulus"]))
