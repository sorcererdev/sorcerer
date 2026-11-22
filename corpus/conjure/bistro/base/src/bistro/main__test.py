from pathlib import Path

from fastapi import FastAPI


def test_importing_main_opens_no_database() -> None:
    import importlib

    import bistro.main

    importlib.reload(bistro.main)
    assert not Path("bistro.sqlite3").exists()


def test_create_makes_the_app(tmp_path: Path) -> None:
    from bistro.main import create

    assert isinstance(create(str(tmp_path / "bistro.sqlite3")), FastAPI)
