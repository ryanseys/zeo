# Encoding "é" (and other Latin letters) to EUC-JP: CRuby reaches JIS X
# 0212 (the SS3 plane -- bytes 8f ab b1); zeo's encoder (encoding_rs's
# WHATWG table, which is decode-only for JIS X 0212) refuses with
# UndefinedConversionError. Related to but distinct from the DECIDED
# ISO-2022-JP divergence in docs/COMPATIBILITY.md. (Found by the
# 2026-08-24 probe sweep.)
p "é".encode("EUC-JP").unpack1("H*")
p "é".encode("EUC-JP").encode("UTF-8") == "é"
