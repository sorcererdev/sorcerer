/** A booking and the table it holds. */
export class Booking {
  readonly table: number;
  constructor(table: number) {
    this.table = table;
  }

  isFor(table: number): boolean {
    return this.table === table;
  }
}
