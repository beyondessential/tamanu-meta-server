import { Chip } from "@mui/material";
import { PRODUCT_LABELS, type Product } from "../types";

const COLORS: Record<Product, "success" | "secondary" | "default"> = {
	tamanu: "success",
	senaite: "secondary",
	canopy: "default",
};

export default function ServerProductChip({ product }: { product: Product }) {
	return (
		<Chip
			size="small"
			variant="outlined"
			color={COLORS[product]}
			label={PRODUCT_LABELS[product]}
		/>
	);
}
