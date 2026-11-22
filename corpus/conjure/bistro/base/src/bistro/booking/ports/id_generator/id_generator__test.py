"""Every IdGenerator adapter keeps the same contract."""

import pytest

from bistro.booking.ports.id_generator.id_generator import IdGenerator
from bistro.booking.ports.id_generator.in_memory_id_generator import InMemoryIdGenerator

ADAPTERS: list[type[IdGenerator]] = [InMemoryIdGenerator]


@pytest.fixture(params=ADAPTERS)
def adapter(request: pytest.FixtureRequest) -> IdGenerator:
    made: IdGenerator = request.param()
    return made


def test_is_a_id_generator(adapter: IdGenerator) -> None:
    assert isinstance(adapter, tuple(ADAPTERS))


def test_next_id_is_new_each_time(adapter: IdGenerator) -> None:
    assert adapter.next_id() != adapter.next_id()
