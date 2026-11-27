import { closeTable } from "./closeTable";
import { Booking } from "./models";

export function testCloseTableKeepsTheOtherTables(): void {
  if (closeTable([new Booking(1)], 2)[0].table !== 1) {
    throw new Error("closeTable dropped another table");
  }
}
