public class OrderEngine {
    public static int ComputeSubtotal(int count, int unit_price) {
        return count * unit_price;
    }

    public static int ApplyDiscount(int subtotal, int discount) {
        return subtotal - discount;
    }
}
