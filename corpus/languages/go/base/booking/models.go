// Package booking holds a booking and the table it holds.
package booking

// Booking is a table held for the night.
type Booking struct {
	Table int
}

// IsFor says whether the booking holds the table.
func (b Booking) IsFor(table int) bool {
	return b.Table == table
}
