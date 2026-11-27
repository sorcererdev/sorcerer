-- The tables of the booking service.
CREATE TABLE bookings (
  id integer PRIMARY KEY,
  table_no integer NOT NULL,
  party_size integer NOT NULL
);

CREATE VIEW open_bookings AS
SELECT id, table_no
FROM bookings
WHERE party_size > 0;

CREATE INDEX bookings_by_table ON bookings (table_no);

CREATE FUNCTION party_size_of(booking integer) RETURNS integer AS $$
  SELECT party_size FROM bookings WHERE id = booking
$$ LANGUAGE sql;
