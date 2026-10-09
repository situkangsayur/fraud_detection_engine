from __future__ import annotations

from pathlib import Path

import pytest

from llm_service.regulations.chunker import chunk_document, section_texts
from llm_service.regulations.extract import ExtractionError, extract_text

REGULATION = """PERATURAN OTORITAS JASA KEUANGAN
NOMOR 99 TAHUN 2030
TENTANG
PENCEGAHAN FRAUD TRANSAKSI DIGITAL

Menimbang : a. bahwa transaksi digital meningkat;
b. bahwa diperlukan pengendalian fraud;

BAB I
KETENTUAN UMUM
Pasal 1
Dalam Peraturan ini yang dimaksud dengan:
1. Fraud adalah tindakan penyimpangan yang disengaja.
2. Transaksi adalah setiap pemindahan dana.

BAB II
PEMANTAUAN TRANSAKSI
Bagian Kesatu
Batas Nominal
Pasal 2
(1) Lembaga wajib memantau transaksi dengan nilai di atas Rp100.000.000,00.
(2) Pemantauan sebagaimana dimaksud pada ayat (1) dilakukan secara real time.
Pasal 3
Lembaga wajib memblokir kartu yang terindikasi carding sebagaimana dimaksud dalam Pasal 2 ayat (1).

PENJELASAN
I. UMUM
Peraturan ini disusun untuk melindungi konsumen.
II. PASAL DEMI PASAL
Pasal 1
Cukup jelas.
Pasal 2
Ayat (1) yang dimaksud nilai adalah nominal per transaksi.

LAMPIRAN I
I. LATAR BELAKANG
Fraud digital meningkat setiap tahun.
II. PEDOMAN
Lembaga menyusun pedoman anti fraud.
"""


def test_structural_units_and_section_paths() -> None:
    chunks = chunk_document(REGULATION)
    keys = [c.key for c in chunks]
    assert keys[0] == "Pembukaan"
    assert [k for k in keys if k.startswith("Pasal")] == ["Pasal 1", "Pasal 2", "Pasal 3"]
    pasal2 = next(c for c in chunks if c.key == "Pasal 2")
    assert pasal2.section == "BAB II PEMANTAUAN TRANSAKSI > Bagian Kesatu Batas Nominal > Pasal 2"
    assert "(1) Lembaga wajib memantau" in pasal2.text and "(2) Pemantauan" in pasal2.text
    assert next(c for c in chunks if c.key == "Pasal 1").title == "KETENTUAN UMUM"
    # in-text references ("dalam Pasal 2 ayat (1).") must not start a new unit
    assert "Pasal 2 ayat (1)" in next(c for c in chunks if c.key == "Pasal 3").text
    # elucidation is kept apart from the body
    assert "Penjelasan Pasal 2" in keys and "Penjelasan Umum" in keys
    assert next(c for c in chunks if c.key == "Penjelasan Pasal 2").section == "Penjelasan > Pasal 2"
    # annex subsections
    assert "Lampiran I I" in keys and "Lampiran I II" in keys
    assert [c.ordinal for c in chunks] == list(range(len(chunks)))


def test_long_pasal_split_on_ayat_and_reassembled() -> None:
    ayat = "\n".join(f"({i}) " + "kewajiban pemantauan transaksi " * 30 for i in range(1, 8))
    text = f"BAB I\nUMUM\nPasal 1\n{ayat}\nPasal 2\nSelesai."
    chunks = chunk_document(text, max_chars=1000)
    parts = [c for c in chunks if c.key.startswith("Pasal 1#")]
    assert len(parts) > 1
    assert all(len(c.text) <= 1000 for c in parts)
    assert all(c.text.lstrip().startswith("(") for c in parts)  # split on ayat boundary
    merged = section_texts(chunks)["Pasal 1"]
    assert merged.count("kewajiban") == 7 * 30


def test_generic_document_without_pasal_uses_headings() -> None:
    text = "# Kebijakan Voucher\nSatu akun satu voucher.\n# Cashback\nMaksimal 3 kali per bulan per perangkat."
    chunks = chunk_document(text)
    assert [c.section for c in chunks] == ["Kebijakan Voucher", "Cashback"]


def test_extract_rejects_unsupported_and_empty() -> None:
    with pytest.raises(ExtractionError):
        extract_text(b"x", "file.exe")
    with pytest.raises(ExtractionError):
        extract_text(b"   ", "empty.txt")


def test_real_pojk_pdf_extraction_and_chunking() -> None:
    pdf = Path(__file__).resolve().parents[4] / "data_regulations" / "pojkatifraud.pdf"
    if not pdf.exists():
        pytest.skip("fixture PDF not available")
    text = extract_text(pdf.read_bytes(), pdf.name)
    assert "OTORITAS JASA KEUANGAN" in text
    chunks = chunk_document(text)
    keys = {c.key.split("#")[0] for c in chunks}
    assert len(chunks) > 50
    assert {"Pasal 1", "Pasal 25", "Penjelasan Pasal 25", "Lampiran I"} <= keys
    assert max(len(c.text) for c in chunks) <= 1800
