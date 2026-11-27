package booking

import "testing"

func TestCloseTableKeepsTheOtherTables(t *testing.T) {
	kept := CloseTable([]Booking{{Table: 1}}, 2)
	if kept[0].Table != 1 {
		t.Fatal("the other table was closed")
	}
}
